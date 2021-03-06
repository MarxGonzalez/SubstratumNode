// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

use multinode_integration_tests_lib::substratum_node::{NodeReference, SubstratumNode};
use multinode_integration_tests_lib::substratum_node_cluster::SubstratumNodeCluster;
use multinode_integration_tests_lib::substratum_real_node::{
    NodeStartupConfigBuilder, SubstratumRealNode,
};
use node_lib::proxy_server::protocol_pack::ServerImpersonator;
use node_lib::proxy_server::server_impersonator_http::ServerImpersonatorHttp;
use node_lib::sub_lib::cryptde::CryptDE;
use node_lib::sub_lib::cryptde_null::CryptDENull;
use node_lib::sub_lib::utils::index_of;
use std::net::IpAddr;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

#[test]
fn end_to_end_routing_test() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let mut bogus_cryptde = CryptDENull::new();
    bogus_cryptde.generate_key_pair();
    let bogus_bootstrap_ref = NodeReference::new(
        bogus_cryptde.public_key().clone(),
        IpAddr::from_str("1.2.3.4").unwrap(),
        vec![1234],
    );
    let first_node = cluster.start_real_node(
        NodeStartupConfigBuilder::standard()
            .neighbor(bogus_bootstrap_ref)
            .build(),
    );

    let nodes = (0..6)
        .map(|_| {
            cluster.start_real_node(
                NodeStartupConfigBuilder::standard()
                    .neighbor(first_node.node_reference())
                    .build(),
            )
        })
        .collect::<Vec<SubstratumRealNode>>();

    thread::sleep(Duration::from_millis(500 * (nodes.len() as u64)));

    let last_node = cluster.start_real_node(
        NodeStartupConfigBuilder::standard()
            .neighbor(nodes.last().unwrap().node_reference())
            .open_firewall_port(8080)
            .build(),
    );

    thread::sleep(Duration::from_millis(500));

    let mut client = last_node.make_client(8080);
    client.send_chunk(Vec::from(
        &b"GET / HTTP/1.1\r\nHost: www.example.com\r\n\r\n"[..],
    ));
    let response = client.wait_for_chunk();

    // If this fails (sporadically) check if there are only 6 nodes in the network and find a better way to wait
    // for it to be 7. There have to be 7 to guarantee an exit node exists for every node in the network
    assert_eq!(
        index_of(
            &response,
            &b"This domain is established to be used for illustrative examples in documents."[..]
        )
        .is_some(),
        true,
        "Actual response:\n{}",
        String::from_utf8(response).unwrap()
    );
}

#[test]
fn http_routing_failure_produces_internal_error_response() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let bootstrap_node = cluster.start_real_node(NodeStartupConfigBuilder::bootstrap().build());
    let originating_node = cluster.start_real_node(
        NodeStartupConfigBuilder::standard()
            .neighbor(bootstrap_node.node_reference())
            .build(),
    );
    thread::sleep(Duration::from_millis(1000));

    let mut client = originating_node.make_client(8080);

    client.send_chunk(Vec::from(
        &b"GET / HTTP/1.1\r\nHost: www.example.com\r\n\r\n"[..],
    ));
    let response = client.wait_for_chunk();

    let expected_response =
        ServerImpersonatorHttp {}.route_query_failure_response("www.example.com");

    assert_eq!(
        &expected_response,
        &response
            .into_iter()
            .take(expected_response.len())
            .collect::<Vec<u8>>(),
    );
}

#[test]
fn tls_routing_failure_produces_internal_error_response() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let bootstrap = cluster.start_real_node(NodeStartupConfigBuilder::bootstrap().build());
    let originating_node = cluster.start_real_node(
        NodeStartupConfigBuilder::standard()
            .neighbor(bootstrap.node_reference())
            .build(),
    );
    let mut client = originating_node.make_client(8443);
    let client_hello = vec![
        0x16, // content_type: Handshake
        0x03, 0x03, // TLS 1.2
        0x00, 0x3F, // length
        0x01, // handshake_type: ClientHello
        0x00, 0x00, 0x3B, // length
        0x00, 0x00, // version: don't care
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, // random: don't care
        0x00, // session_id_length
        0x00, 0x00, // cipher_suites_length
        0x00, // compression_methods_length
        0x00, 0x13, // extensions_length
        0x00, 0x00, // extension_type: server_name
        0x00, 0x0F, // extension_length
        0x00, 0x0D, // server_name_list_length
        0x00, // server_name_type
        0x00, 0x0A, // server_name_length
        's' as u8, 'e' as u8, 'r' as u8, 'v' as u8, 'e' as u8, 'r' as u8, '.' as u8, 'c' as u8,
        'o' as u8, 'm' as u8, // server_name
    ];

    client.send_chunk(client_hello);
    let response = client.wait_for_chunk();

    assert_eq!(
        vec![
            0x15, // alert
            0x03, 0x03, // TLS 1.2
            0x00, 0x02, // packet length
            0x02, // fatal alert
            0x50, // internal_error alert
        ],
        response
    )
}

#[test]
fn multiple_stream_zero_hop_test() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let zero_hop_node = cluster.start_real_node(NodeStartupConfigBuilder::zero_hop().build());
    let mut one_client = zero_hop_node.make_client(8080);
    let mut another_client = zero_hop_node.make_client(8080);

    one_client.send_chunk(Vec::from(
        &b"GET / HTTP/1.1\r\nHost: www.example.com\r\n\r\n"[..],
    ));
    another_client.send_chunk(Vec::from(
        &b"GET / HTTP/1.1\r\nHost: www.fallingfalling.com\r\n\r\n"[..],
    ));

    let one_response = one_client.wait_for_chunk();
    let another_response = another_client.wait_for_chunk();

    assert_eq!(
        index_of(
            &one_response,
            &b"This domain is established to be used for illustrative examples in documents."[..]
        )
        .is_some(),
        true,
        "Actual response:\n{}",
        String::from_utf8(one_response).unwrap()
    );
    assert_eq!(
        index_of(
            &another_response,
            &b"FALLING FALLING .COM BY RAFAEL ROZENDAAL"[..]
        )
        .is_some(),
        true,
        "Actual response:\n{}",
        String::from_utf8(another_response).unwrap()
    );
}
