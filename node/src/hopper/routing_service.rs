// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.
use super::live_cores_package::LiveCoresPackage;
use crate::sub_lib::accountant::ReportRoutingServiceProvidedMessage;
use crate::sub_lib::cryptde::CryptData;
use crate::sub_lib::cryptde::CryptdecError;
use crate::sub_lib::cryptde::PlainData;
use crate::sub_lib::cryptde::{encodex, CryptDE};
use crate::sub_lib::dispatcher::Component;
use crate::sub_lib::dispatcher::Endpoint;
use crate::sub_lib::dispatcher::InboundClientData;
use crate::sub_lib::hop::LiveHop;
use crate::sub_lib::hopper::{ExpiredCoresPackage, HopperSubs, MessageType};
use crate::sub_lib::logger::Logger;
use crate::sub_lib::neighborhood::NeighborhoodSubs;
use crate::sub_lib::proxy_client::ProxyClientSubs;
use crate::sub_lib::proxy_server::ProxyServerSubs;
use crate::sub_lib::stream_handler_pool::TransmitDataMsg;
use crate::sub_lib::wallet::Wallet;
use actix::Recipient;
use std::borrow::Borrow;
use std::net::{IpAddr, SocketAddr};

pub struct RoutingServiceSubs {
    pub proxy_client_subs: ProxyClientSubs,
    pub proxy_server_subs: ProxyServerSubs,
    pub neighborhood_subs: NeighborhoodSubs,
    pub hopper_subs: HopperSubs,
    pub to_dispatcher: Recipient<TransmitDataMsg>,
    pub to_accountant_routing: Recipient<ReportRoutingServiceProvidedMessage>,
}

pub struct RoutingService {
    cryptde: &'static dyn CryptDE,
    is_bootstrap_node: bool,
    routing_service_subs: RoutingServiceSubs,
    per_routing_service: u64,
    per_routing_byte: u64,
    logger: Logger,
}

impl RoutingService {
    pub fn new(
        cryptde: &'static dyn CryptDE,
        is_bootstrap_node: bool,
        routing_service_subs: RoutingServiceSubs,
        per_routing_service: u64,
        per_routing_byte: u64,
    ) -> RoutingService {
        RoutingService {
            cryptde,
            is_bootstrap_node,
            routing_service_subs,
            per_routing_service,
            per_routing_byte,
            logger: Logger::new("RoutingService"),
        }
    }

    pub fn route(&self, ibcd: InboundClientData) {
        let data_size = ibcd.data.len();
        self.logger.debug(format!(
            "Received {} bytes of InboundClientData ({}) from Dispatcher",
            data_size, ibcd.peer_addr
        ));
        let peer_addr = ibcd.peer_addr;
        let last_data = ibcd.last_data;
        let ibcd_but_data = ibcd.clone_but_data();
        let live_package = match self.decrypt_and_deserialize_lcp(ibcd) {
            Ok(package) => package,
            Err(_) => return, // log already written
        };

        let next_hop = match live_package.route.next_hop(self.cryptde.borrow()) {
            Ok(hop) => hop,
            Err(e) => {
                self.logger
                    .error(format!("Invalid {}-byte CORES package: {:?}", data_size, e));
                return;
            }
        };

        if self.should_route_data(peer_addr, next_hop.component) {
            self.route_data(
                peer_addr.ip(),
                next_hop,
                live_package,
                last_data,
                &ibcd_but_data,
            );
        }
    }

    fn route_data(
        &self,
        sender_ip: IpAddr,
        next_hop: LiveHop,
        live_package: LiveCoresPackage,
        last_data: bool,
        ibcd_but_data: &InboundClientData,
    ) {
        if (next_hop.component == Component::Hopper) && (!self.is_destined_for_here(&next_hop)) {
            self.route_data_externally(live_package, next_hop.consuming_wallet, last_data);
        } else {
            self.route_data_internally(next_hop.component, sender_ip, live_package, ibcd_but_data)
        }
    }

    fn is_destined_for_here(&self, next_hop: &LiveHop) -> bool {
        &next_hop.public_key == self.cryptde.public_key()
    }

    fn route_data_internally(
        &self,
        component: Component,
        immediate_neighbor_ip: IpAddr,
        live_package: LiveCoresPackage,
        ibcd_but_data: &InboundClientData,
    ) {
        if component == Component::Hopper {
            self.route_data_around_again(live_package, ibcd_but_data)
        } else {
            self.route_data_to_peripheral_component(component, immediate_neighbor_ip, live_package)
        }
    }

    fn route_data_around_again(
        &self,
        live_package: LiveCoresPackage,
        ibcd_but_data: &InboundClientData,
    ) {
        let (_, next_lcp) = match live_package.to_next_live(self.cryptde) {
            Ok(x) => x,
            Err(_) => unimplemented!(),
        };
        let payload = match encodex(self.cryptde, &self.cryptde.public_key(), &next_lcp) {
            Ok(lcp) => lcp,
            Err(_) => unimplemented!(),
        };
        let inbound_client_data = InboundClientData {
            peer_addr: ibcd_but_data.peer_addr,
            reception_port: ibcd_but_data.reception_port,
            last_data: ibcd_but_data.last_data,
            is_clandestine: ibcd_but_data.is_clandestine,
            sequence_number: ibcd_but_data.sequence_number,
            data: payload.into(),
        };
        self.routing_service_subs
            .hopper_subs
            .from_dispatcher
            .try_send(inbound_client_data)
            .expect("Hopper is dead");
    }

    fn route_data_to_peripheral_component(
        &self,
        component: Component,
        immediate_neighbor_ip: IpAddr,
        live_package: LiveCoresPackage,
    ) {
        let data_len = live_package.payload.len();
        let expired_package =
            match live_package.to_expired(immediate_neighbor_ip, self.cryptde.borrow()) {
                Ok(pkg) => pkg,
                Err(e) => {
                    self.logger.error(format!(
                        "Couldn't expire CORES package with {}-byte payload: {:?}",
                        data_len, e
                    ));
                    return;
                }
            };
        self.logger.trace(format!(
            "Forwarding ExpiredCoresPackage to {:?}: {:?}",
            component, expired_package
        ));
        match (component, expired_package.payload) {
            (Component::ProxyClient, MessageType::ClientRequest(client_request)) => self
                .routing_service_subs
                .proxy_client_subs
                .from_hopper
                .try_send(ExpiredCoresPackage::new(
                    expired_package.immediate_neighbor_ip,
                    expired_package.consuming_wallet,
                    expired_package.remaining_route,
                    client_request,
                    expired_package.payload_len,
                ))
                .expect("Proxy Client is dead"),
            (Component::ProxyServer, MessageType::ClientResponse(client_reponse)) => self
                .routing_service_subs
                .proxy_server_subs
                .from_hopper
                .try_send(ExpiredCoresPackage::new(
                    expired_package.immediate_neighbor_ip,
                    expired_package.consuming_wallet,
                    expired_package.remaining_route,
                    client_reponse,
                    expired_package.payload_len,
                ))
                .expect("Proxy Server is dead"),
            (Component::ProxyServer, MessageType::DnsResolveFailed(dns_resolve_failure)) => self
                .routing_service_subs
                .proxy_server_subs
                .dns_failure_from_hopper
                .try_send(ExpiredCoresPackage::new(
                    expired_package.immediate_neighbor_ip,
                    expired_package.consuming_wallet,
                    expired_package.remaining_route,
                    dns_resolve_failure,
                    expired_package.payload_len,
                ))
                .expect("Proxy Server is dead"),
            (Component::Neighborhood, MessageType::Gossip(gossip)) => self
                .routing_service_subs
                .neighborhood_subs
                .from_hopper
                .try_send(ExpiredCoresPackage::new(
                    expired_package.immediate_neighbor_ip,
                    expired_package.consuming_wallet,
                    expired_package.remaining_route,
                    gossip,
                    expired_package.payload_len,
                ))
                .expect("Neighborhood is dead"),
            (destination, payload) => self.logger.error(format!(
                "Attempt to send invalid combination {:?} to {:?}",
                payload, destination
            )),
        };
    }

    fn route_data_externally(
        &self,
        live_package: LiveCoresPackage,
        consuming_wallet_opt: Option<Wallet>,
        last_data: bool,
    ) {
        let payload_size = live_package.payload.len();
        match consuming_wallet_opt {
            Some(consuming_wallet) => {
                self.routing_service_subs
                    .to_accountant_routing
                    .try_send(ReportRoutingServiceProvidedMessage {
                        consuming_wallet,
                        payload_size,
                        service_rate: self.per_routing_service,
                        byte_rate: self.per_routing_byte,
                    })
                    .expect("Accountant is dead");
            }
            None => {
                self.logger.error(format!(
                    "Refusing to route CORES package with {}-byte payload without consuming wallet",
                    payload_size
                ));
                return;
            }
        }

        let transmit_msg = match self.to_transmit_data_msg(live_package, last_data) {
            // crashpoint - need to figure out how to bubble up different kinds of errors, or just log and return
            Err(_) => unimplemented!(),
            Ok(m) => m,
        };

        self.logger.debug(format!(
            "Relaying {}-byte LiveCoresPackage Dispatcher inside a TransmitDataMsg",
            transmit_msg.data.len()
        ));
        self.routing_service_subs
            .to_dispatcher
            .try_send(transmit_msg)
            .expect("Dispatcher is dead");
    }

    fn to_transmit_data_msg(
        &self,
        live_package: LiveCoresPackage,
        last_data: bool,
    ) -> Result<TransmitDataMsg, CryptdecError> {
        let (next_hop, next_live_package) = match live_package.to_next_live(self.cryptde.borrow()) {
            // crashpoint - log error and return None?
            Err(_) => unimplemented!(),
            Ok(p) => p,
        };
        let next_live_package_ser = match serde_cbor::ser::to_vec(&next_live_package) {
            // crashpoint - log error and return None?
            Err(_) => unimplemented!(),
            Ok(p) => p,
        };
        let next_live_package_enc = match self.cryptde.encode(
            &next_hop.public_key,
            &PlainData::new(&next_live_package_ser[..]),
        ) {
            // crashpoint - log error and return None?
            Err(_) => unimplemented!(),
            Ok(p) => p,
        };
        Ok(TransmitDataMsg {
            endpoint: Endpoint::Key(next_hop.public_key),
            last_data,
            data: next_live_package_enc.into(),
            sequence_number: None,
        })
    }

    fn should_route_data(&self, peer_addr: SocketAddr, component: Component) -> bool {
        if component == Component::Neighborhood {
            true
        } else if self.is_bootstrap_node {
            self.logger.error(format!(
                "Request from {} for Bootstrap Node to route data to {:?}: rejected",
                peer_addr, component
            ));
            false
        } else {
            true
        }
    }

    fn decrypt_and_deserialize_lcp(&self, ibcd: InboundClientData) -> Result<LiveCoresPackage, ()> {
        let decrypted_package = match self.cryptde.decode(&CryptData::new(&ibcd.data[..])) {
            Ok(package) => package,
            Err(e) => {
                self.logger.error(format!(
                    "Couldn't decrypt CORES package from {}-byte buffer: {:?}",
                    ibcd.data.len(),
                    e
                ));
                return Err(());
            }
        };
        let live_package =
            match serde_cbor::de::from_slice::<LiveCoresPackage>(decrypted_package.as_slice()) {
                Ok(package) => package,
                Err(e) => {
                    self.logger
                        .error(format!("Couldn't deserialize CORES package: {}", e));
                    return Err(());
                }
            };
        return Ok(live_package);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neighborhood::gossip::{Gossip, GossipBuilder};
    use crate::sub_lib::accountant::ReportRoutingServiceProvidedMessage;
    use crate::sub_lib::cryptde::{encodex, PublicKey};
    use crate::sub_lib::cryptde_null::CryptDENull;
    use crate::sub_lib::hopper::IncipientCoresPackage;
    use crate::sub_lib::hopper::MessageType;
    use crate::sub_lib::proxy_client::{ClientResponsePayload, DnsResolveFailure};
    use crate::sub_lib::proxy_server::ClientRequestPayload;
    use crate::sub_lib::route::Route;
    use crate::sub_lib::route::RouteSegment;
    use crate::sub_lib::wallet::Wallet;
    use crate::test_utils::logging::init_test_logging;
    use crate::test_utils::logging::TestLogHandler;
    use crate::test_utils::recorder::make_recorder;
    use crate::test_utils::recorder::peer_actors_builder;
    use crate::test_utils::test_utils::make_meaningless_message_type;
    use crate::test_utils::test_utils::route_to_proxy_server;
    use crate::test_utils::test_utils::{cryptde, make_request_payload};
    use crate::test_utils::test_utils::{make_meaningless_stream_key, route_to_proxy_client};
    use crate::test_utils::test_utils::{make_response_payload, rate_pack_routing};
    use crate::test_utils::test_utils::{rate_pack_routing_byte, route_from_proxy_client};
    use actix::System;
    use std::net::SocketAddr;
    use std::str::FromStr;

    #[test]
    fn dns_resolution_failures_are_reported_to_the_proxy_server() {
        init_test_logging();
        let cryptde = cryptde();
        let route = route_to_proxy_server(&cryptde.public_key(), cryptde);
        let stream_key = make_meaningless_stream_key();
        let dns_resolve_failure = DnsResolveFailure { stream_key };
        let lcp = LiveCoresPackage::new(
            route,
            encodex(
                cryptde,
                &cryptde.public_key(),
                &MessageType::DnsResolveFailed(dns_resolve_failure.clone()),
            )
            .unwrap(),
        );
        let data_enc = encodex(cryptde, &cryptde.public_key(), &lcp).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            sequence_number: None,
            last_data: false,
            is_clandestine: false,
            data: data_enc.into(),
        };
        let (proxy_server, proxy_server_awaiter, proxy_server_recording) = make_recorder();

        let system = System::new("dns_resolution_failures_are_reported_to_the_proxy_server");
        let peer_actors = peer_actors_builder().proxy_server(proxy_server).build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            100,
            200,
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();

        proxy_server_awaiter.await_message_count(1);
        let recordings = proxy_server_recording.lock().unwrap();
        let message = recordings.get_record::<ExpiredCoresPackage<DnsResolveFailure>>(0);
        assert_eq!(dns_resolve_failure, message.payload);
    }

    #[test]
    fn logs_and_ignores_message_that_cannot_be_decoded() {
        init_test_logging();
        let cryptde = cryptde();
        let route = route_from_proxy_client(&cryptde.public_key(), cryptde);
        let lcp = LiveCoresPackage::new(
            route,
            encodex(cryptde, &cryptde.public_key(), &[42u8]).unwrap(),
        );
        let data_enc = encodex(cryptde, &cryptde.public_key(), &lcp).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            sequence_number: None,
            last_data: false,
            is_clandestine: false,
            data: data_enc.into(),
        };
        let peer_actors = peer_actors_builder().build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            100,
            200,
        );
        subject.route(inbound_client_data);
        TestLogHandler::new().await_log_matching(
            "Couldn't expire CORES package with \\d+-byte payload: .*",
            1000,
        );
    }

    #[test]
    fn logs_and_ignores_message_that_had_invalid_destination() {
        init_test_logging();
        let cryptde = cryptde();
        let route = route_from_proxy_client(&cryptde.public_key(), cryptde);
        let payload = GossipBuilder::empty();
        let lcp = LiveCoresPackage::new(
            route,
            encodex(
                cryptde,
                &cryptde.public_key(),
                &MessageType::Gossip(payload),
            )
            .unwrap(),
        );
        let data_enc = encodex(cryptde, &cryptde.public_key(), &lcp).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            sequence_number: None,
            last_data: false,
            is_clandestine: false,
            data: data_enc.into(),
        };
        let peer_actors = peer_actors_builder().build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            100,
            200,
        );
        subject.route(inbound_client_data);
        TestLogHandler::new()
            .await_log_matching("Attempt to send invalid combination .* to .*", 1000);
    }

    #[test]
    fn converts_live_message_to_expired_for_proxy_client() {
        let cryptde = cryptde();
        let (component, _, component_recording_arc) = make_recorder();
        let route = route_to_proxy_client(&cryptde.public_key(), cryptde);
        let payload = make_request_payload(0, cryptde);
        let lcp = LiveCoresPackage::new(
            route,
            encodex::<MessageType>(cryptde, &cryptde.public_key(), &payload.clone().into())
                .unwrap(),
        );
        let lcp_a = lcp.clone();
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            sequence_number: None,
            last_data: true,
            is_clandestine: false,
            data: data_enc.into(),
        };

        let system = System::new("converts_live_message_to_expired_for_proxy_client");
        let peer_actors = peer_actors_builder().proxy_client(component).build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            0,
            0,
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();
        let component_recording = component_recording_arc.lock().unwrap();
        let record = component_recording.get_record::<ExpiredCoresPackage<ClientRequestPayload>>(0);
        let expected_ecp = lcp_a
            .to_expired(IpAddr::from_str("1.2.3.4").unwrap(), cryptde)
            .unwrap();
        assert_eq!(
            record.immediate_neighbor_ip,
            expected_ecp.immediate_neighbor_ip
        );
        assert_eq!(record.consuming_wallet, expected_ecp.consuming_wallet);
        assert_eq!(record.remaining_route, expected_ecp.remaining_route);
        assert_eq!(record.payload, payload);
        assert_eq!(record.payload_len, expected_ecp.payload_len);
    }

    #[test]
    fn converts_live_message_to_expired_for_proxy_server() {
        let cryptde = cryptde();
        let (component, _, component_recording_arc) = make_recorder();
        let route = route_to_proxy_server(&cryptde.public_key(), cryptde);
        let payload = make_response_payload(0, cryptde);
        let lcp = LiveCoresPackage::new(
            route,
            encodex::<MessageType>(cryptde, &cryptde.public_key(), &payload.clone().into())
                .unwrap(),
        );
        let lcp_a = lcp.clone();
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.3.2.4:5678").unwrap(),
            reception_port: None,
            last_data: false,
            is_clandestine: true,
            sequence_number: None,
            data: data_enc.into(),
        };

        let system = System::new("converts_live_message_to_expired_for_proxy_server");
        let peer_actors = peer_actors_builder().proxy_server(component).build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            0,
            0,
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();
        let component_recording = component_recording_arc.lock().unwrap();
        let record =
            component_recording.get_record::<ExpiredCoresPackage<ClientResponsePayload>>(0);
        let expected_ecp = lcp_a
            .to_expired(IpAddr::from_str("1.3.2.4").unwrap(), cryptde)
            .unwrap();
        assert_eq!(
            record.immediate_neighbor_ip,
            expected_ecp.immediate_neighbor_ip
        );
        assert_eq!(record.consuming_wallet, expected_ecp.consuming_wallet);
        assert_eq!(record.remaining_route, expected_ecp.remaining_route);
        assert_eq!(record.payload, payload);
        assert_eq!(record.payload_len, expected_ecp.payload_len);
    }

    #[test]
    fn converts_live_message_to_expired_for_neighborhood() {
        let cryptde = cryptde();
        let (component, _, component_recording_arc) = make_recorder();
        let mut route = Route::one_way(
            RouteSegment::new(
                vec![&cryptde.public_key(), &cryptde.public_key()],
                Component::Neighborhood,
            ),
            cryptde,
            None,
        )
        .unwrap();
        route.shift(cryptde).unwrap();
        let payload = GossipBuilder::empty();
        let lcp = LiveCoresPackage::new(
            route,
            encodex::<MessageType>(cryptde, &cryptde.public_key(), &payload.clone().into())
                .unwrap(),
        );
        let lcp_a = lcp.clone();
        let data_enc = encodex(cryptde, &cryptde.public_key(), &lcp).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.3.2.4:5678").unwrap(),
            reception_port: None,
            last_data: false,
            is_clandestine: true,
            sequence_number: None,
            data: data_enc.into(),
        };

        let system = System::new("converts_live_message_to_expired_for_neighborhood");
        let peer_actors = peer_actors_builder().neighborhood(component).build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            0,
            0,
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();
        let component_recording = component_recording_arc.lock().unwrap();
        let record = component_recording.get_record::<ExpiredCoresPackage<Gossip>>(0);
        let expected_ecp = lcp_a
            .to_expired(IpAddr::from_str("1.3.2.4").unwrap(), cryptde)
            .unwrap();
        assert_eq!(
            record.immediate_neighbor_ip,
            expected_ecp.immediate_neighbor_ip
        );
        assert_eq!(record.consuming_wallet, expected_ecp.consuming_wallet);
        assert_eq!(record.remaining_route, expected_ecp.remaining_route);
        assert_eq!(record.payload, payload);
        assert_eq!(record.payload_len, expected_ecp.payload_len);
    }

    #[test]
    fn passes_on_inbound_client_data_not_meant_for_this_node() {
        let cryptde = cryptde();
        let consuming_wallet = Wallet::new("wallet");
        let (dispatcher, _, dispatcher_recording_arc) = make_recorder();
        let (accountant, _, accountant_recording_arc) = make_recorder();
        let next_key = PublicKey::new(&[65, 65, 65]);
        let route = Route::one_way(
            RouteSegment::new(
                vec![&cryptde.public_key(), &next_key],
                Component::Neighborhood,
            ),
            cryptde,
            Some(consuming_wallet.clone()),
        )
        .unwrap();
        let payload = PlainData::new(&b"abcd"[..]);
        let lcp = LiveCoresPackage::new(route, cryptde.encode(&next_key, &payload).unwrap());
        let lcp_a = lcp.clone();
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            last_data: true,
            is_clandestine: false, // TODO This should probably be true
            sequence_number: None,
            data: data_enc.into(),
        };

        let system = System::new("passes_on_inbound_client_data_not_meant_for_this_node");
        let peer_actors = peer_actors_builder()
            .dispatcher(dispatcher)
            .accountant(accountant)
            .build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            rate_pack_routing(103),
            rate_pack_routing_byte(103),
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();

        let dispatcher_recording = dispatcher_recording_arc.lock().unwrap();
        let record = dispatcher_recording.get_record::<TransmitDataMsg>(0);
        let expected_lcp = lcp_a.to_next_live(cryptde).unwrap().1;
        let expected_lcp_ser = PlainData::new(&serde_cbor::ser::to_vec(&expected_lcp).unwrap());
        let expected_lcp_enc = cryptde.encode(&next_key, &expected_lcp_ser).unwrap();
        assert_eq!(
            *record,
            TransmitDataMsg {
                endpoint: Endpoint::Key(next_key.clone()),
                last_data: true,
                sequence_number: None,
                data: expected_lcp_enc.into(),
            }
        );
        let accountant_recording = accountant_recording_arc.lock().unwrap();
        let message = accountant_recording.get_record::<ReportRoutingServiceProvidedMessage>(0);
        assert_eq!(
            *message,
            ReportRoutingServiceProvidedMessage {
                consuming_wallet,
                payload_size: lcp.payload.len(),
                service_rate: rate_pack_routing(103),
                byte_rate: rate_pack_routing_byte(103),
            }
        )
    }

    #[test]
    fn reprocesses_inbound_client_data_meant_for_this_node_and_destined_for_hopper() {
        let cryptde = cryptde();
        let consuming_wallet = Wallet::new("wallet");
        let (hopper, _, hopper_recording_arc) = make_recorder();
        let route = Route::one_way(
            RouteSegment::new(
                vec![&cryptde.public_key(), &cryptde.public_key()],
                Component::Neighborhood,
            ),
            cryptde,
            Some(consuming_wallet.clone()),
        )
        .unwrap();
        let payload = PlainData::new(&b"abcd"[..]);
        let lcp = LiveCoresPackage::new(
            route,
            cryptde.encode(&cryptde.public_key(), &payload).unwrap(),
        );
        let lcp_a = lcp.clone();
        let data_enc = encodex(cryptde, &cryptde.public_key(), &lcp).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            last_data: true,
            is_clandestine: true,
            sequence_number: None,
            data: data_enc.into(),
        };

        let system = System::new(
            "reprocesses_inbound_client_data_meant_for_this_node_and_destined_for_hopper",
        );
        let peer_actors = peer_actors_builder().hopper(hopper).build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            rate_pack_routing(103),
            rate_pack_routing_byte(103),
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();

        let hopper_recording = hopper_recording_arc.lock().unwrap();
        let record = hopper_recording.get_record::<InboundClientData>(0);
        let expected_lcp = lcp_a.to_next_live(cryptde).unwrap().1;
        let expected_lcp_enc = encodex(cryptde, &cryptde.public_key(), &expected_lcp).unwrap();
        assert_eq!(
            *record,
            InboundClientData {
                peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
                reception_port: None,
                last_data: true,
                is_clandestine: true,
                sequence_number: None,
                data: expected_lcp_enc.into()
            }
        );
    }

    #[test]
    fn refuses_data_for_proxy_client_if_is_bootstrap_node() {
        init_test_logging();
        let cryptde = cryptde();
        let (component, _, component_recording_arc) = make_recorder();
        let route = route_to_proxy_client(&cryptde.public_key(), cryptde);
        let payload = PlainData::new(&b"abcd"[..]);
        let lcp = LiveCoresPackage::new(
            route,
            cryptde.encode(&cryptde.public_key(), &payload).unwrap(),
        );
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5679").unwrap(),
            reception_port: None,
            last_data: false,
            is_clandestine: false,
            sequence_number: None,
            data: data_enc.into(),
        };
        let system = System::new("refuses_data_for_proxy_client_if_is_bootstrap_node");
        let peer_actors = peer_actors_builder().proxy_client(component).build();
        let subject = RoutingService::new(
            cryptde,
            true,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            0,
            0,
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();
        TestLogHandler::new().exists_log_containing(
            "ERROR: RoutingService: Request from 1.2.3.4:5679 for Bootstrap Node to route data to ProxyClient: rejected",
        );
        let component_recording = component_recording_arc.lock().unwrap();
        assert_eq!(0, component_recording.len());
    }

    #[test]
    fn refuses_data_for_proxy_server_if_is_bootstrap_node() {
        init_test_logging();
        let cryptde = cryptde();
        let (component, _, component_recording_arc) = make_recorder();
        let route = route_to_proxy_server(&cryptde.public_key(), cryptde);
        let payload = PlainData::new(&b"abcd"[..]);
        let lcp = LiveCoresPackage::new(
            route,
            cryptde.encode(&cryptde.public_key(), &payload).unwrap(),
        );
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            sequence_number: None,
            last_data: false,
            is_clandestine: false,
            data: data_enc.into(),
        };
        let system = System::new("refuses_data_for_proxy_server_if_is_bootstrap_node");
        let peer_actors = peer_actors_builder().proxy_server(component).build();
        let subject = RoutingService::new(
            cryptde,
            true,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            0,
            0,
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();
        TestLogHandler::new().exists_log_containing(
            "ERROR: RoutingService: Request from 1.2.3.4:5678 for Bootstrap Node to route data to ProxyServer: rejected",
        );
        let component_recording = component_recording_arc.lock().unwrap();
        assert_eq!(0, component_recording.len());
    }

    #[test]
    fn refuses_data_for_hopper_if_is_bootstrap_node() {
        init_test_logging();
        let cryptde = cryptde();
        let (component, _, component_recording_arc) = make_recorder();
        let consuming_wallet = Wallet::new("wallet");
        let route = Route::one_way(
            RouteSegment::new(
                vec![&cryptde.public_key(), &cryptde.public_key()],
                Component::Hopper,
            ),
            cryptde,
            Some(consuming_wallet),
        )
        .unwrap();
        let payload = PlainData::new(&b"abcd"[..]);
        let lcp = LiveCoresPackage::new(
            route,
            cryptde.encode(&cryptde.public_key(), &payload).unwrap(),
        );
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5681").unwrap(),
            reception_port: None,
            last_data: false,
            is_clandestine: true,
            sequence_number: None,
            data: data_enc.into(),
        };
        let system = System::new("refuses_data_for_hopper_if_is_bootstrap_node");
        let peer_actors = peer_actors_builder().hopper(component).build();
        let subject = RoutingService::new(
            cryptde,
            true,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            0,
            0,
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();
        TestLogHandler::new().exists_log_containing(
            "ERROR: RoutingService: Request from 1.2.3.4:5681 for Bootstrap Node to route data to Hopper: rejected",
        );
        let component_recording = component_recording_arc.lock().unwrap();
        assert_eq!(0, component_recording.len());
    }

    #[test]
    fn accepts_data_for_neighborhood_if_is_bootstrap_node() {
        init_test_logging();
        let cryptde = cryptde();
        let (neighborhood, _, neighborhood_recording_arc) = make_recorder();
        let mut route = Route::one_way(
            RouteSegment::new(
                vec![&cryptde.public_key(), &cryptde.public_key()],
                Component::Neighborhood,
            ),
            cryptde,
            None,
        )
        .unwrap();
        route.shift(cryptde).unwrap();
        let payload = GossipBuilder::empty();
        let encrypted_payload =
            encodex::<MessageType>(cryptde, &cryptde.public_key(), &payload.clone().into())
                .unwrap();
        let lcp = LiveCoresPackage::new(route, encrypted_payload.clone());
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            last_data: false,
            is_clandestine: true,
            sequence_number: None,
            data: data_enc.into(),
        };
        let system = System::new("accepts_data_for_neighborhood_if_is_bootstrap_node");
        let peer_actors = peer_actors_builder().neighborhood(neighborhood).build();
        let subject = RoutingService::new(
            cryptde,
            true,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            0,
            0,
        );

        subject.route(inbound_client_data);

        System::current().stop();
        system.run();
        let neighborhood_recording = neighborhood_recording_arc.lock().unwrap();
        let message = neighborhood_recording.get_record::<ExpiredCoresPackage<Gossip>>(0);
        assert_eq!(message.clone().payload, payload);
        assert_eq!(
            message.clone().immediate_neighbor_ip,
            IpAddr::from_str("1.2.3.4").unwrap()
        );
        TestLogHandler::new().exists_no_log_containing(
            "ERROR: RoutingService: Request for Bootstrap Node to route data to Neighborhood: rejected",
        );
    }

    #[test]
    fn route_logs_and_ignores_cores_package_that_demands_routing_without_consuming_wallet() {
        init_test_logging();
        let cryptde = cryptde();
        let origin_key = PublicKey::new(&[1, 2]);
        let origin_cryptde = CryptDENull::from(&origin_key);
        let destination_key = PublicKey::new(&[3, 4]);
        let payload = make_meaningless_message_type();
        let route = Route::one_way(
            RouteSegment::new(
                vec![&origin_key, &cryptde.public_key(), &destination_key],
                Component::ProxyClient,
            ),
            &origin_cryptde,
            None,
        )
        .unwrap();
        let icp =
            IncipientCoresPackage::new(&origin_cryptde, route, payload, &destination_key).unwrap();
        let (lcp, _) = LiveCoresPackage::from_incipient(icp, &origin_cryptde).unwrap();
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            last_data: true,
            is_clandestine: true,
            sequence_number: None,
            data: data_enc.into(),
        };
        let system = System::new(
            "route_logs_and_ignores_cores_package_that_demands_routing_without_consuming_wallet",
        );
        let (proxy_client, _, proxy_client_recording_arc) = make_recorder();
        let (proxy_server, _, proxy_server_recording_arc) = make_recorder();
        let (neighborhood, _, neighborhood_recording_arc) = make_recorder();
        let (dispatcher, _, dispatcher_recording_arc) = make_recorder();
        let peer_actors = peer_actors_builder()
            .proxy_client(proxy_client)
            .proxy_server(proxy_server)
            .neighborhood(neighborhood)
            .dispatcher(dispatcher)
            .build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            100,
            200,
        );

        subject.route(inbound_client_data);

        System::current().stop_with_code(0);
        system.run();
        TestLogHandler::new().exists_log_matching(
            "ERROR: RoutingService: Refusing to route CORES package with \\d+-byte payload without consuming wallet",
        );
        assert_eq!(proxy_client_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(proxy_server_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(neighborhood_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(dispatcher_recording_arc.lock().unwrap().len(), 0);
    }

    #[test]
    fn route_logs_and_ignores_inbound_client_data_that_doesnt_deserialize_properly() {
        init_test_logging();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            last_data: true,
            is_clandestine: true,
            sequence_number: None,
            data: vec![],
        };
        let system = System::new("consume_logs_error_when_given_bad_input_data");
        let (proxy_client, _, proxy_client_recording_arc) = make_recorder();
        let (proxy_server, _, proxy_server_recording_arc) = make_recorder();
        let (neighborhood, _, neighborhood_recording_arc) = make_recorder();
        let (dispatcher, _, dispatcher_recording_arc) = make_recorder();
        let peer_actors = peer_actors_builder()
            .proxy_client(proxy_client)
            .proxy_server(proxy_server)
            .neighborhood(neighborhood)
            .dispatcher(dispatcher)
            .build();
        let subject = RoutingService::new(
            cryptde(),
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            100,
            200,
        );

        subject.route(inbound_client_data);

        System::current().stop_with_code(0);
        system.run();
        TestLogHandler::new().exists_log_containing(
            "ERROR: RoutingService: Couldn't decrypt CORES package from 0-byte buffer: EmptyData",
        );
        assert_eq!(proxy_client_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(proxy_server_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(neighborhood_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(dispatcher_recording_arc.lock().unwrap().len(), 0);
    }

    #[test]
    fn route_logs_and_ignores_invalid_live_cores_package() {
        init_test_logging();
        let cryptde = cryptde();
        let lcp = LiveCoresPackage::new(Route { hops: vec![] }, CryptData::new(&[]));
        let data_ser = PlainData::new(&serde_cbor::ser::to_vec(&lcp).unwrap()[..]);
        let data_enc = cryptde.encode(&cryptde.public_key(), &data_ser).unwrap();
        let inbound_client_data = InboundClientData {
            peer_addr: SocketAddr::from_str("1.2.3.4:5678").unwrap(),
            reception_port: None,
            last_data: true,
            is_clandestine: true,
            sequence_number: None,
            data: data_enc.into(),
        };
        let system = System::new("consume_logs_error_when_given_bad_input_data");
        let (proxy_client, _, proxy_client_recording_arc) = make_recorder();
        let (proxy_server, _, proxy_server_recording_arc) = make_recorder();
        let (neighborhood, _, neighborhood_recording_arc) = make_recorder();
        let (dispatcher, _, dispatcher_recording_arc) = make_recorder();
        let peer_actors = peer_actors_builder()
            .proxy_client(proxy_client)
            .proxy_server(proxy_server)
            .neighborhood(neighborhood)
            .dispatcher(dispatcher)
            .build();
        let subject = RoutingService::new(
            cryptde,
            false,
            RoutingServiceSubs {
                proxy_client_subs: peer_actors.proxy_client,
                proxy_server_subs: peer_actors.proxy_server,
                neighborhood_subs: peer_actors.neighborhood,
                hopper_subs: peer_actors.hopper,
                to_dispatcher: peer_actors.dispatcher.from_dispatcher_client,
                to_accountant_routing: peer_actors.accountant.report_routing_service_provided,
            },
            100,
            200,
        );

        subject.route(inbound_client_data);

        System::current().stop_with_code(0);
        system.run();
        TestLogHandler::new().exists_log_containing(
            "ERROR: RoutingService: Invalid 36-byte CORES package: EmptyRoute",
        );
        assert_eq!(proxy_client_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(proxy_server_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(neighborhood_recording_arc.lock().unwrap().len(), 0);
        assert_eq!(dispatcher_recording_arc.lock().unwrap().len(), 0);
    }
}
