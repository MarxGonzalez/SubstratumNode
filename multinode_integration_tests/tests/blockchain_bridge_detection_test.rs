// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

use multinode_integration_tests_lib::substratum_node::SubstratumNode;
use multinode_integration_tests_lib::substratum_node::SubstratumNodeUtils;
use multinode_integration_tests_lib::substratum_node_cluster::SubstratumNodeCluster;
use multinode_integration_tests_lib::substratum_real_node::NodeStartupConfigBuilder;
use node_lib::sub_lib::cryptde::{CryptDE, PlainData};
use node_lib::sub_lib::cryptde_null::CryptDENull;
use regex::escape;
use std::time::Duration;

#[test]
fn blockchain_bridge_logs_when_started() {
    let mut cluster = SubstratumNodeCluster::start().unwrap();
    let private_key = "0011223300112233001122330011223300112233001122330011223300112233";
    let subject = cluster.start_real_node(
        NodeStartupConfigBuilder::zero_hop()
            .consuming_private_key(private_key)
            .build(),
    );
    let hash = CryptDENull::new().hash(&PlainData::new(private_key.as_bytes()));

    let escaped_pattern = escape(&format!(
        "DEBUG: BlockchainBridge: Received BindMessage; consuming private key that hashes to {:?}",
        hash
    ));
    SubstratumNodeUtils::wrote_log_containing(
        subject.name(),
        &escaped_pattern,
        Duration::from_millis(1000),
    )
}
