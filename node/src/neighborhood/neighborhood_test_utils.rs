// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.
#![cfg(test)]

use super::neighborhood_database::NeighborhoodDatabase;
use super::node_record::NodeRecord;
use crate::neighborhood::neighborhood::Neighborhood;
use crate::neighborhood::node_record::NodeRecordInner;
use crate::sub_lib::cryptde::PublicKey;
use crate::sub_lib::cryptde::{CryptDE, PlainData};
use crate::sub_lib::cryptde_null::CryptDENull;
use crate::sub_lib::neighborhood::{NeighborhoodConfig, NodeDescriptor};
use crate::sub_lib::node_addr::NodeAddr;
use crate::sub_lib::wallet::Wallet;
use crate::test_utils::test_utils::cryptde;
use crate::test_utils::test_utils::rate_pack;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::str::FromStr;

pub fn make_node_record(n: u16, has_ip: bool, is_bootstrap_node: bool) -> NodeRecord {
    let a = ((n / 1000) % 10) as u8;
    let b = ((n / 100) % 10) as u8;
    let c = ((n / 10) % 10) as u8;
    let d = (n % 10) as u8;
    let key = PublicKey::new(&[a, b, c, d]);
    let ip_addr = IpAddr::V4(Ipv4Addr::new(a, b, c, d));
    let node_addr = NodeAddr::new(&ip_addr, &vec![n % 10000]);

    NodeRecord::new_for_tests(
        &key,
        if has_ip { Some(&node_addr) } else { None },
        n as u64,
        is_bootstrap_node,
    )
}

pub fn make_global_cryptde_node_record(
    n: u16,
    has_ip: bool,
    is_bootstrap_node: bool,
) -> NodeRecord {
    let mut node_record = make_node_record(n, has_ip, is_bootstrap_node);
    node_record.inner.public_key = cryptde().public_key().clone();
    node_record.resign();
    node_record
}

pub fn db_from_node(node: &NodeRecord) -> NeighborhoodDatabase {
    NeighborhoodDatabase::new(
        node.public_key(),
        &node.node_addr_opt().unwrap_or(NodeAddr::new(
            &IpAddr::from_str("200.200.200.200").unwrap(),
            &vec![200],
        )),
        node.earning_wallet(),
        node.rate_pack().clone(),
        node.is_bootstrap_node(),
        &CryptDENull::from(node.public_key()),
    )
}

pub fn neighborhood_from_nodes(
    root: &NodeRecord,
    neighbor_opt: Option<&NodeRecord>,
) -> Neighborhood {
    let cryptde = cryptde();
    if root.public_key() != cryptde.public_key() {
        panic!("Neighborhood must be built on root node with public key from cryptde()");
    }
    Neighborhood::new(
        cryptde,
        NeighborhoodConfig {
            neighbor_configs: match neighbor_opt {
                None => vec![],
                Some(neighbor) => vec![NodeDescriptor {
                    public_key: neighbor.public_key().clone(),
                    node_addr: neighbor
                        .node_addr_opt()
                        .expect("Neighbor has to have NodeAddr"),
                }],
            },
            is_bootstrap_node: root.is_bootstrap_node(),
            local_ip_addr: root
                .node_addr_opt()
                .expect("Root has to have NodeAddr")
                .ip_addr(),
            clandestine_port_list: root.node_addr_opt().unwrap().ports(),
            earning_wallet: root.earning_wallet(),
            consuming_wallet: Some(Wallet::new("consuming")),
            rate_pack: root.rate_pack().clone(),
        },
    )
}

impl NodeRecord {
    pub fn earning_wallet_from_key(public_key: &PublicKey) -> Wallet {
        let mut result = String::from("0x");
        for i in public_key.as_slice() {
            result.push_str(&format!("{:x}", i));
        }
        Wallet { address: result }
    }

    pub fn consuming_wallet_from_key(public_key: &PublicKey) -> Option<Wallet> {
        let mut result = String::from("0x");
        let mut reversed_public_key_data = Vec::from(public_key.as_slice());
        reversed_public_key_data.reverse();
        for i in &reversed_public_key_data {
            result.push_str(&format!("{:x}", i));
        }
        Some(Wallet { address: result })
    }

    pub fn new_for_tests(
        public_key: &PublicKey,
        node_addr_opt: Option<&NodeAddr>,
        base_rate: u64,
        is_bootstrap_node: bool,
    ) -> NodeRecord {
        let mut node_record = NodeRecord::new(
            public_key,
            NodeRecord::earning_wallet_from_key(public_key),
            rate_pack(base_rate),
            is_bootstrap_node,
            0,
            &CryptDENull::from(public_key),
        );
        if let Some(node_addr) = node_addr_opt {
            node_record.set_node_addr(node_addr).unwrap();
        }
        node_record.signed_gossip =
            PlainData::from(serde_cbor::ser::to_vec(&node_record.inner).unwrap());
        node_record.regenerate_signed_gossip(&CryptDENull::from(&public_key));
        node_record
    }

    pub fn resign(&mut self) {
        let cryptde = CryptDENull::from(self.public_key());
        self.regenerate_signed_gossip(&cryptde);
    }
}

impl PartialEq for NodeRecord {
    fn eq(&self, other: &NodeRecord) -> bool {
        if self.inner != other.inner {
            return false;
        }
        if self.metadata != other.metadata {
            return false;
        }
        if self.signature != other.signature {
            return false;
        }
        let self_nri: NodeRecordInner =
            serde_cbor::de::from_slice(self.signed_gossip.as_slice()).unwrap();
        let other_nri: NodeRecordInner =
            serde_cbor::de::from_slice(other.signed_gossip.as_slice()).unwrap();
        self_nri == other_nri
    }
}

impl NeighborhoodDatabase {
    // These methods are intended for use only in tests. Do not use them in production code.
    pub fn add_arbitrary_half_neighbor(
        &mut self,
        node_key: &PublicKey,
        new_neighbor: &PublicKey,
    ) -> bool {
        if self.has_half_neighbor(node_key, new_neighbor) {
            false
        } else {
            let node_ref = self.node_by_key_mut(node_key).unwrap();
            node_ref.add_half_neighbor_key(new_neighbor.clone());
            node_ref.resign();
            true
        }
    }

    pub fn add_arbitrary_full_neighbor(
        &mut self,
        node_key: &PublicKey,
        new_neighbor: &PublicKey,
    ) -> bool {
        if self.has_full_neighbor(node_key, new_neighbor) {
            false
        } else {
            let over = self.add_arbitrary_half_neighbor(node_key, new_neighbor);
            let back = self.add_arbitrary_half_neighbor(new_neighbor, node_key);
            over || back
        }
    }

    pub fn remove_arbitrary_half_neighbor(
        &mut self,
        node_key: &PublicKey,
        neighbor_key: &PublicKey,
    ) -> bool {
        if let Some(node) = self.node_by_key_mut(node_key) {
            if node.remove_half_neighbor_key(neighbor_key) {
                node.resign();
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn resign_node(&mut self, public_key: &PublicKey) {
        let node_record = {
            let mut node_record = self.node_by_key(public_key).unwrap().clone();
            node_record.resign();
            node_record
        };
        let node_ref = self.node_by_key_mut(public_key).unwrap();
        node_ref.signed_gossip = node_record.signed_gossip;
        node_ref.signature = node_record.signature;
    }
}
