// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.
use crate::neighborhood::gossip::Gossip;
use crate::sub_lib::cryptde::PublicKey;
use crate::sub_lib::dispatcher::Component;
use crate::sub_lib::hopper::ExpiredCoresPackage;
use crate::sub_lib::node_addr::NodeAddr;
use crate::sub_lib::peer_actors::BindMessage;
use crate::sub_lib::route::Route;
use crate::sub_lib::stream_handler_pool::DispatcherNodeQueryResponse;
use crate::sub_lib::stream_handler_pool::TransmitDataMsg;
use crate::sub_lib::wallet::Wallet;
use actix::Message;
use actix::Recipient;
use serde_derive::{Deserialize, Serialize};
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::str::FromStr;

pub const SENTINEL_IP_OCTETS: [u8; 4] = [255, 255, 255, 255];

pub const DEFAULT_RATE_PACK: RatePack = RatePack {
    routing_byte_rate: 100,
    routing_service_rate: 10000,
    exit_byte_rate: 101,
    exit_service_rate: 10001,
};

pub const ZERO_RATE_PACK: RatePack = RatePack {
    routing_byte_rate: 0,
    routing_service_rate: 0,
    exit_byte_rate: 0,
    exit_service_rate: 0,
};

pub fn sentinel_ip_addr() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(
        SENTINEL_IP_OCTETS[0],
        SENTINEL_IP_OCTETS[1],
        SENTINEL_IP_OCTETS[2],
        SENTINEL_IP_OCTETS[3],
    ))
}

#[derive(Clone, PartialEq, Debug)]
pub struct NodeDescriptor {
    pub public_key: PublicKey,
    pub node_addr: NodeAddr,
}

impl FromStr for NodeDescriptor {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pieces: Vec<&str> = s.splitn(2, ":").collect();

        if pieces.len() != 2 {
            return Err(String::from(s));
        }

        let public_key = match base64::decode(pieces[0]) {
            Ok(key) => PublicKey::new(&key),
            Err(_) => return Err(String::from(s)),
        };

        if public_key.is_empty() {
            return Err(String::from(s));
        }

        let node_addr = match NodeAddr::from_str(&pieces[1]) {
            Ok(node_addr) => node_addr,
            Err(_) => return Err(String::from(s)),
        };

        Ok(NodeDescriptor {
            public_key,
            node_addr,
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct NeighborhoodConfig {
    pub neighbor_configs: Vec<NodeDescriptor>,
    pub is_bootstrap_node: bool,
    pub local_ip_addr: IpAddr,
    pub clandestine_port_list: Vec<u16>,
    pub earning_wallet: Wallet,
    pub consuming_wallet: Option<Wallet>,
    pub rate_pack: RatePack,
}

impl NeighborhoodConfig {
    pub fn is_decentralized(&self) -> bool {
        !self.neighbor_configs.is_empty()
            && (self.local_ip_addr != sentinel_ip_addr())
            && !self.clandestine_port_list.is_empty()
    }
}

#[derive(Clone)]
pub struct NeighborhoodSubs {
    pub bind: Recipient<BindMessage>,
    pub bootstrap: Recipient<BootstrapNeighborhoodNowMessage>,
    pub node_query: Recipient<NodeQueryMessage>,
    pub route_query: Recipient<RouteQueryMessage>,
    pub update_node_record_metadata: Recipient<NodeRecordMetadataMessage>,
    pub from_hopper: Recipient<ExpiredCoresPackage<Gossip>>,
    pub dispatcher_node_query: Recipient<DispatcherNodeQueryMessage>,
    pub remove_neighbor: Recipient<RemoveNeighborMessage>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeQueryResponseMetadata {
    pub public_key: PublicKey,
    pub node_addr_opt: Option<NodeAddr>,
    pub rate_pack: RatePack,
}

impl NodeQueryResponseMetadata {
    pub fn new(
        public_key: PublicKey,
        node_addr_opt: Option<NodeAddr>,
        rate_pack: RatePack,
    ) -> NodeQueryResponseMetadata {
        NodeQueryResponseMetadata {
            public_key,
            node_addr_opt,
            rate_pack,
        }
    }
}

#[derive(Message, Clone)]
pub struct BootstrapNeighborhoodNowMessage {}

#[derive(Debug, PartialEq, Clone)]
pub enum NodeQueryMessage {
    IpAddress(IpAddr),
    PublicKey(PublicKey),
}

impl Message for NodeQueryMessage {
    type Result = Option<NodeQueryResponseMetadata>;
}

#[derive(Message, Clone)]
pub struct DispatcherNodeQueryMessage {
    pub query: NodeQueryMessage,
    pub context: TransmitDataMsg,
    pub recipient: Recipient<DispatcherNodeQueryResponse>,
}

#[derive(PartialEq, Clone, Debug, Copy)]
pub enum TargetType {
    Bootstrap,
    Standard,
}

#[derive(PartialEq, Debug)]
pub struct RouteQueryMessage {
    pub target_type: TargetType,
    pub target_key_opt: Option<PublicKey>,
    pub target_component: Component,
    pub minimum_hop_count: usize,
    pub return_component_opt: Option<Component>,
}

impl Message for RouteQueryMessage {
    type Result = Option<RouteQueryResponse>;
}

impl RouteQueryMessage {
    pub fn data_indefinite_route_request(minimum_hop_count: usize) -> RouteQueryMessage {
        RouteQueryMessage {
            target_type: TargetType::Standard,
            target_key_opt: None,
            target_component: Component::ProxyClient,
            minimum_hop_count,
            return_component_opt: Some(Component::ProxyServer),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum ExpectedService {
    Routing(PublicKey, Wallet, RatePack),
    Exit(PublicKey, Wallet, RatePack),
    Nothing,
}

#[derive(PartialEq, Debug, Clone)]
pub enum ExpectedServices {
    OneWay(Vec<ExpectedService>),
    RoundTrip(Vec<ExpectedService>, Vec<ExpectedService>, u32),
}

#[derive(PartialEq, Debug, Clone)]
pub struct RouteQueryResponse {
    pub route: Route,
    pub expected_services: ExpectedServices,
}

#[derive(PartialEq, Debug, Message, Clone)]
pub struct RemoveNeighborMessage {
    pub public_key: PublicKey,
}

#[derive(PartialEq, Debug, Message, Clone)]
pub enum NodeRecordMetadataMessage {
    Desirable(PublicKey, bool),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct RatePack {
    pub routing_byte_rate: u64,
    pub routing_service_rate: u64,
    pub exit_byte_rate: u64,
    pub exit_service_rate: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    pub fn rate_pack(base_rate: u64) -> RatePack {
        RatePack {
            routing_byte_rate: base_rate + 1,
            routing_service_rate: base_rate + 2,
            exit_byte_rate: base_rate + 3,
            exit_service_rate: base_rate + 4,
        }
    }

    #[test]
    fn node_descriptor_from_str_requires_two_pieces_to_a_configuration() {
        let result = NodeDescriptor::from_str("only_one_piece");

        assert_eq!(Err(String::from("only_one_piece")), result);
    }

    #[test]
    fn node_descriptor_from_str_complains_about_bad_base_64() {
        let result = NodeDescriptor::from_str("bad_key:1.2.3.4:1234,2345");

        assert_eq!(Err(String::from("bad_key:1.2.3.4:1234,2345")), result);
    }

    #[test]
    fn node_descriptor_from_str_complains_about_blank_public_key() {
        let result = NodeDescriptor::from_str(":1.2.3.4:1234,2345");

        assert_eq!(Err(String::from(":1.2.3.4:1234,2345")), result);
    }

    #[test]
    fn node_descriptor_from_str_complains_about_bad_node_addr() {
        let result = NodeDescriptor::from_str("R29vZEtleQ==:BadNodeAddr");

        assert_eq!(Err(String::from("R29vZEtleQ==:BadNodeAddr")), result);
    }

    #[test]
    fn node_descriptor_from_str_handles_the_happy_path() {
        let result = NodeDescriptor::from_str("R29vZEtleQ:1.2.3.4:1234,2345,3456");

        assert_eq!(
            NodeDescriptor {
                public_key: PublicKey::new(b"GoodKey"),
                node_addr: NodeAddr::new(
                    &IpAddr::from_str("1.2.3.4").unwrap(),
                    &vec!(1234, 2345, 3456),
                )
            },
            result.unwrap()
        )
    }

    #[test]
    fn data_indefinite_route_request() {
        let result = RouteQueryMessage::data_indefinite_route_request(2);

        assert_eq!(
            result,
            RouteQueryMessage {
                target_type: TargetType::Standard,
                target_key_opt: None,
                target_component: Component::ProxyClient,
                minimum_hop_count: 2,
                return_component_opt: Some(Component::ProxyServer),
            }
        );
    }

    #[test]
    fn neighborhood_config_is_not_decentralized_if_there_are_no_neighbor_configs() {
        let subject = NeighborhoodConfig {
            neighbor_configs: vec![],
            earning_wallet: Wallet::new("router"),
            consuming_wallet: Some(Wallet::new("consumer")),
            rate_pack: rate_pack(100),
            is_bootstrap_node: false,
            local_ip_addr: IpAddr::from_str("1.2.3.4").unwrap(),
            clandestine_port_list: vec![1234],
        };

        let result = subject.is_decentralized();

        assert_eq!(result, false);
    }

    #[test]
    fn neighborhood_config_is_not_decentralized_if_the_sentinel_ip_address_is_used() {
        let subject = NeighborhoodConfig {
            neighbor_configs: vec![NodeDescriptor {
                public_key: PublicKey::new(&b"key"[..]),
                node_addr: NodeAddr::new(&IpAddr::from_str("2.3.4.5").unwrap(), &vec![2345]),
            }],
            earning_wallet: Wallet::new("router"),
            consuming_wallet: Some(Wallet::new("consumer")),
            rate_pack: rate_pack(100),
            is_bootstrap_node: false,
            local_ip_addr: sentinel_ip_addr(),
            clandestine_port_list: vec![1234],
        };

        let result = subject.is_decentralized();

        assert_eq!(result, false);
    }

    #[test]
    fn neighborhood_config_is_not_decentralized_if_there_are_no_clandestine_ports() {
        let subject = NeighborhoodConfig {
            neighbor_configs: vec![NodeDescriptor {
                public_key: PublicKey::new(&b"key"[..]),
                node_addr: NodeAddr::new(&IpAddr::from_str("2.3.4.5").unwrap(), &vec![2345]),
            }],
            earning_wallet: Wallet::new("router"),
            consuming_wallet: Some(Wallet::new("consumer")),
            rate_pack: rate_pack(100),
            is_bootstrap_node: false,
            local_ip_addr: IpAddr::from_str("1.2.3.4").unwrap(),
            clandestine_port_list: vec![],
        };

        let result = subject.is_decentralized();

        assert_eq!(result, false);
    }

    #[test]
    fn neighborhood_config_is_decentralized_if_neighbor_config_and_local_ip_addr_and_clandestine_port(
    ) {
        let subject = NeighborhoodConfig {
            neighbor_configs: vec![NodeDescriptor {
                public_key: PublicKey::new(&b"key"[..]),
                node_addr: NodeAddr::new(&IpAddr::from_str("2.3.4.5").unwrap(), &vec![2345]),
            }],
            earning_wallet: Wallet::new("router"),
            consuming_wallet: Some(Wallet::new("consumer")),
            rate_pack: rate_pack(100),
            is_bootstrap_node: false,
            local_ip_addr: IpAddr::from_str("1.2.3.4").unwrap(),
            clandestine_port_list: vec![1234],
        };

        let result = subject.is_decentralized();

        assert_eq!(result, true);
    }
}
