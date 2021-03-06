// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

use crate::neighborhood::gossip::GossipNodeRecord;
use crate::neighborhood::neighborhood::AccessibleGossipRecord;
use crate::neighborhood::neighborhood_database::{NeighborhoodDatabase, NeighborhoodDatabaseError};
use crate::sub_lib::cryptde::{CryptDE, CryptData, PlainData, PublicKey};
use crate::sub_lib::neighborhood::RatePack;
use crate::sub_lib::node_addr::NodeAddr;
use crate::sub_lib::wallet::Wallet;
use serde_derive::{Deserialize, Serialize};
use std::collections::btree_set::BTreeSet;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::iter::FromIterator;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct NodeRecordInner {
    pub public_key: PublicKey,
    pub earning_wallet: Wallet,
    pub rate_pack: RatePack,
    pub is_bootstrap_node: bool,
    pub neighbors: BTreeSet<PublicKey>,
    pub version: u32,
}

impl TryFrom<GossipNodeRecord> for NodeRecordInner {
    type Error = String;

    fn try_from(gnr: GossipNodeRecord) -> Result<Self, Self::Error> {
        match serde_cbor::from_slice(gnr.signed_data.as_slice()) {
            Ok(inner) => Ok(inner),
            Err(e) => Err(format!("{:?}", e)),
        }
    }
}

impl TryFrom<&GossipNodeRecord> for NodeRecordInner {
    type Error = String;

    fn try_from(gnr_addr_ref: &GossipNodeRecord) -> Result<Self, Self::Error> {
        NodeRecordInner::try_from(gnr_addr_ref.clone())
    }
}

#[derive(Clone, Debug)]
pub struct NodeRecord {
    pub inner: NodeRecordInner,
    pub metadata: NodeRecordMetadata,
    pub signed_gossip: PlainData,
    pub signature: CryptData,
}

impl NodeRecord {
    pub fn new(
        public_key: &PublicKey,
        earning_wallet: Wallet,
        rate_pack: RatePack,
        is_bootstrap_node: bool,
        version: u32,
        cryptde: &CryptDE,
    ) -> NodeRecord {
        let mut node_record = NodeRecord {
            metadata: NodeRecordMetadata::new(),
            inner: NodeRecordInner {
                public_key: public_key.clone(),
                earning_wallet,
                rate_pack,
                is_bootstrap_node,
                neighbors: BTreeSet::new(),
                version,
            },
            signed_gossip: PlainData::new(&[]),
            signature: CryptData::new(&[]),
        };
        node_record.regenerate_signed_gossip(cryptde);
        node_record
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.inner.public_key
    }

    pub fn node_addr_opt(&self) -> Option<NodeAddr> {
        self.metadata.node_addr_opt.clone()
    }

    pub fn is_bootstrap_node(&self) -> bool {
        self.inner.is_bootstrap_node
    }

    pub fn is_not_bootstrap_node(&self) -> bool {
        !self.is_bootstrap_node()
    }

    pub fn set_node_addr(
        &mut self,
        node_addr: &NodeAddr,
    ) -> Result<bool, NeighborhoodDatabaseError> {
        match self.metadata.node_addr_opt {
            Some(ref inner_node_addr) if node_addr == inner_node_addr => Ok(false),
            Some(ref inner_node_addr) => Err(NeighborhoodDatabaseError::NodeAddrAlreadySet(
                inner_node_addr.clone(),
            )),
            None => {
                self.metadata.node_addr_opt = Some(node_addr.clone());
                Ok(true)
            }
        }
    }

    pub fn unset_node_addr(&mut self) {
        self.metadata.node_addr_opt = None
    }

    pub fn half_neighbor_keys(&self) -> HashSet<&PublicKey> {
        HashSet::from_iter(self.inner.neighbors.iter())
    }

    pub fn has_half_neighbor(&self, key: &PublicKey) -> bool {
        self.inner.neighbors.contains(key)
    }

    pub fn add_half_neighbor_key(&mut self, key: PublicKey) {
        self.inner.neighbors.insert(key);
    }

    pub fn add_half_neighbor_keys(&mut self, keys: Vec<PublicKey>) {
        keys.into_iter().for_each(|k| self.add_half_neighbor_key(k));
    }

    pub fn remove_half_neighbor_key(&mut self, key: &PublicKey) -> bool {
        self.inner.neighbors.remove(key)
    }

    pub fn clear_half_neighbors(&mut self) {
        self.inner.neighbors.clear();
    }

    // Keep in mind that this is a O(n^2) method
    pub fn full_neighbors<'a>(&self, db: &'a NeighborhoodDatabase) -> Vec<&'a NodeRecord> {
        let keys = self.full_neighbor_keys(db);
        keys.into_iter()
            .map(|k| {
                db.node_by_key(k)
                    .expect(format!("Node with key {} magically disappeared", k).as_str())
            })
            .collect()
    }

    // Keep in mind that this is a O(n^2) method
    pub fn full_neighbor_keys(&self, db: &NeighborhoodDatabase) -> HashSet<&PublicKey> {
        self.half_neighbor_keys()
            .into_iter()
            .filter(|k| {
                if let Some(node_record_ref) = db.node_by_key(k) {
                    let result = node_record_ref.is_not_bootstrap_node()
                        && node_record_ref.has_half_neighbor(self.public_key());
                    result
                } else {
                    false
                }
            })
            .collect()
    }

    pub fn has_full_neighbor(&self, db: &NeighborhoodDatabase, key: &PublicKey) -> bool {
        if !self.half_neighbor_keys().contains(key) {
            return false;
        }
        match db.node_by_key(key) {
            Some(neighbor) => {
                neighbor.half_neighbor_keys().contains(self.public_key())
                    && !neighbor.is_bootstrap_node()
            }
            None => false,
        }
    }

    pub fn regenerate_signed_gossip(&mut self, cryptde: &dyn CryptDE) {
        self.signed_gossip =
            PlainData::from(serde_cbor::ser::to_vec(&self.inner).expect("Serialization failed"));
        self.signature = match cryptde.sign(&self.signed_gossip) {
            Ok(sig) => sig,
            Err(e) => unimplemented!("Signing error: {:?}", e),
        }
    }

    pub fn signed_gossip(&self) -> &PlainData {
        &self.signed_gossip
    }

    pub fn signature(&self) -> &CryptData {
        &self.signature
    }

    pub fn version(&self) -> u32 {
        self.inner.version
    }

    pub fn increment_version(&mut self) {
        self.inner.version += 1;
    }

    pub fn set_version(&mut self, value: u32) {
        self.inner.version = value;
    }

    pub fn earning_wallet(&self) -> Wallet {
        self.inner.earning_wallet.clone()
    }

    pub fn set_earning_wallet(&mut self, earning_wallet: Wallet) -> bool {
        let change = if self.inner.earning_wallet == earning_wallet {
            false
        } else {
            self.inner.earning_wallet = earning_wallet;
            true
        };
        change
    }

    pub fn rate_pack(&self) -> &RatePack {
        &self.inner.rate_pack
    }

    pub fn is_desirable(&self) -> bool {
        self.metadata.desirable
    }

    pub fn set_desirable(&mut self, is_desirable: bool) {
        self.metadata.desirable = is_desirable
    }
}

impl From<AccessibleGossipRecord> for NodeRecord {
    fn from(agr: AccessibleGossipRecord) -> Self {
        let mut node_record = NodeRecord {
            inner: agr.inner,
            metadata: NodeRecordMetadata::new(),
            signed_gossip: agr.signed_gossip,
            signature: agr.signature,
        };
        node_record.metadata.node_addr_opt = agr.node_addr_opt;
        node_record
    }
}

impl From<&AccessibleGossipRecord> for NodeRecord {
    fn from(agr_ref: &AccessibleGossipRecord) -> Self {
        let agr = agr_ref.clone();
        NodeRecord::from(agr)
    }
}

impl TryFrom<&GossipNodeRecord> for NodeRecord {
    type Error = String;

    fn try_from(gnr: &GossipNodeRecord) -> Result<Self, Self::Error> {
        let inner = NodeRecordInner::try_from(gnr)?;
        let mut node_record = NodeRecord {
            inner,
            metadata: NodeRecordMetadata::new(),
            signed_gossip: gnr.signed_data.clone(),
            signature: gnr.signature.clone(),
        };
        node_record.metadata.node_addr_opt = gnr.node_addr_opt.clone();
        Ok(node_record)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeRecordMetadata {
    pub desirable: bool,
    pub node_addr_opt: Option<NodeAddr>,
}

impl NodeRecordMetadata {
    pub fn new() -> NodeRecordMetadata {
        NodeRecordMetadata {
            desirable: true,
            node_addr_opt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::neighborhood_test_utils::make_node_record;
    use super::*;
    use crate::neighborhood::gossip::GossipBuilder;
    use crate::neighborhood::neighborhood_test_utils::db_from_node;
    use crate::sub_lib::cryptde_null::CryptDENull;
    use crate::test_utils::test_utils::{assert_contains, cryptde, rate_pack};
    use std::net::IpAddr;
    use std::str::FromStr;

    #[test]
    fn can_create_a_node_record_from_a_reference() {
        let mut expected_node_record = make_node_record(1234, true, true);
        expected_node_record.set_version(6);
        expected_node_record.resign();
        let mut db = db_from_node(&make_node_record(2345, true, false));
        db.add_node(expected_node_record.clone()).unwrap();
        let builder = GossipBuilder::new(&db).node(expected_node_record.public_key(), true);

        let actual_node_record =
            NodeRecord::try_from(builder.build().node_records.first().unwrap()).unwrap();

        assert_eq!(expected_node_record, actual_node_record);
    }

    #[test]
    fn set_node_addr_works_once_but_not_twice() {
        let mut subject = make_node_record(1234, false, false);
        assert_eq!(subject.node_addr_opt(), None);
        let first_node_addr = NodeAddr::new(&IpAddr::from_str("4.3.2.1").unwrap(), &vec![4321]);
        let result = subject.set_node_addr(&first_node_addr);
        assert_eq!(result, Ok(true));
        assert_eq!(subject.node_addr_opt(), Some(first_node_addr.clone()));
        let second_node_addr = NodeAddr::new(&IpAddr::from_str("5.4.3.2").unwrap(), &vec![5432]);
        let result = subject.set_node_addr(&second_node_addr);
        assert_eq!(
            result,
            Err(NeighborhoodDatabaseError::NodeAddrAlreadySet(
                first_node_addr.clone()
            ))
        );
        assert_eq!(subject.node_addr_opt(), Some(first_node_addr));
    }

    #[test]
    fn set_node_addr_works_twice_if_the_new_address_is_the_same_as_the_old() {
        let mut subject = make_node_record(1234, false, false);
        assert_eq!(subject.node_addr_opt(), None);
        let first_node_addr = NodeAddr::new(&IpAddr::from_str("4.3.2.1").unwrap(), &vec![4321]);
        let result = subject.set_node_addr(&first_node_addr);
        assert_eq!(result, Ok(true));
        assert_eq!(subject.node_addr_opt(), Some(first_node_addr.clone()));
        let second_node_addr = NodeAddr::new(&IpAddr::from_str("4.3.2.1").unwrap(), &vec![4321]);
        let result = subject.set_node_addr(&second_node_addr);
        assert_eq!(result, Ok(false));
        assert_eq!(subject.node_addr_opt(), Some(first_node_addr));
    }

    #[test]
    fn unset_node_addr() {
        let mut subject = make_node_record(1234, true, false);

        subject.unset_node_addr();

        assert_eq!(None, subject.node_addr_opt());
    }

    #[test]
    fn half_neighbor_manipulation() {
        let mut subject = make_node_record(1234, false, false);

        assert_eq!(subject.half_neighbor_keys().is_empty(), true);

        let neighbor_one = PublicKey::new(&b"one"[..]);
        let neighbor_two = PublicKey::new(&b"two"[..]);
        let neighbor_three = PublicKey::new(&b"three"[..]);
        let neighbor_four = PublicKey::new(&b"four"[..]);

        subject.add_half_neighbor_key(neighbor_one.clone());
        subject.add_half_neighbor_keys(vec![neighbor_two.clone(), neighbor_three.clone()]);
        subject.add_half_neighbor_key(neighbor_one.clone());

        assert_eq!(
            subject.half_neighbor_keys(),
            vec![&neighbor_one, &neighbor_two, &neighbor_three]
                .into_iter()
                .collect::<HashSet<&PublicKey>>()
        );
        assert_eq!(subject.has_half_neighbor(&neighbor_two), true);
        assert_eq!(subject.has_half_neighbor(&neighbor_four), false);

        subject.remove_half_neighbor_key(&neighbor_two);

        assert_eq!(
            subject.half_neighbor_keys(),
            vec![&neighbor_one, &neighbor_three]
                .into_iter()
                .collect::<HashSet<&PublicKey>>()
        );

        subject.clear_half_neighbors();

        assert_eq!(subject.half_neighbor_keys(), HashSet::new());
    }

    #[test]
    fn full_neighbor_exploration() {
        let this_node = make_node_record(1000, true, false);
        let mut database = db_from_node(&this_node);
        let half_neighbor_one = make_node_record(1001, true, false);
        let half_neighbor_two = make_node_record(1002, true, false);
        let mut half_neighbor_reverse = make_node_record(1003, true, false);
        let half_neighbor_bootstrap = make_node_record(1004, true, true);
        let mut full_neighbor_one = make_node_record(1005, true, false);
        let mut full_neighbor_two = make_node_record(1006, true, false);
        let mut full_neighbor_bootstrap = make_node_record(1007, true, true);
        let disconnected = make_node_record(1008, false, false);
        let nonexistent = make_node_record(1009, false, false);

        {
            let this_node = database.root_mut();
            this_node.add_half_neighbor_keys(vec![
                half_neighbor_one.public_key().clone(),
                half_neighbor_two.public_key().clone(),
                half_neighbor_bootstrap.public_key().clone(),
                full_neighbor_one.public_key().clone(),
                full_neighbor_two.public_key().clone(),
                full_neighbor_bootstrap.public_key().clone(),
            ]);
        }
        let this_node = database.root();
        vec![
            &mut half_neighbor_reverse,
            &mut full_neighbor_one,
            &mut full_neighbor_two,
            &mut full_neighbor_bootstrap,
        ]
        .into_iter()
        .for_each(|n| n.add_half_neighbor_key(this_node.public_key().clone()));

        vec![
            &half_neighbor_one,
            &half_neighbor_two,
            &half_neighbor_reverse,
            &half_neighbor_bootstrap,
            &full_neighbor_one,
            &full_neighbor_two,
            &full_neighbor_bootstrap,
            &disconnected,
        ]
        .into_iter()
        .for_each(|n| {
            database.add_node(n.clone()).unwrap();
        });

        let this_node = database.root();
        let full_neighbors = this_node.full_neighbors(&database);
        assert_contains(&full_neighbors, &&full_neighbor_one);
        assert_contains(&full_neighbors, &&full_neighbor_two);
        assert_eq!(full_neighbors.len(), 2);
        assert_eq!(
            this_node.full_neighbor_keys(&database),
            HashSet::from_iter(
                vec![
                    full_neighbor_one.public_key(),
                    full_neighbor_two.public_key()
                ]
                .into_iter()
            )
        );
        assert_eq!(
            this_node.has_full_neighbor(&database, full_neighbor_one.public_key()),
            true
        );
        assert_eq!(
            this_node.has_full_neighbor(&database, full_neighbor_bootstrap.public_key()),
            false
        );
        assert_eq!(
            this_node.has_full_neighbor(&database, half_neighbor_one.public_key()),
            false
        );
        assert_eq!(
            this_node.has_full_neighbor(&database, half_neighbor_bootstrap.public_key()),
            false
        );
        assert_eq!(
            this_node.has_full_neighbor(&database, half_neighbor_reverse.public_key()),
            false
        );
        assert_eq!(
            this_node.has_full_neighbor(&database, disconnected.public_key()),
            false
        );
        assert_eq!(
            this_node.has_full_neighbor(&database, nonexistent.public_key()),
            false
        );
    }

    #[test]
    fn node_record_partial_eq() {
        let earning_wallet = Wallet::new("wallet");
        let exemplar = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            true,
            0,
            cryptde(),
        );
        let duplicate = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            true,
            0,
            cryptde(),
        );
        let mut with_neighbor = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            true,
            0,
            cryptde(),
        );
        let mod_key = NodeRecord::new(
            &PublicKey::new(&b"kope"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            true,
            0,
            cryptde(),
        );
        with_neighbor.add_half_neighbor_key(mod_key.public_key().clone());
        let mut mod_node_addr = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            true,
            0,
            cryptde(),
        );
        mod_node_addr
            .set_node_addr(&NodeAddr::new(
                &IpAddr::from_str("1.2.3.5").unwrap(),
                &vec![1234],
            ))
            .unwrap();
        let mod_earning_wallet = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            Wallet::new("booga"),
            rate_pack(100),
            true,
            0,
            cryptde(),
        );
        let mod_rate_pack = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(200),
            true,
            0,
            cryptde(),
        );
        let mod_is_bootstrap = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            false,
            0,
            cryptde(),
        );
        let mut mod_signed_gossip = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            true,
            0,
            cryptde(),
        );
        mod_signed_gossip.signed_gossip = mod_is_bootstrap.signed_gossip.clone();
        let mut mod_signature = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            true,
            0,
            cryptde(),
        );
        mod_signature.signature = CryptData::new(&[]);
        let mod_version = NodeRecord::new(
            &PublicKey::new(&b"poke"[..]),
            earning_wallet.clone(),
            rate_pack(100),
            true,
            1,
            cryptde(),
        );

        assert_eq!(exemplar, exemplar);
        assert_eq!(exemplar, duplicate);
        assert_ne!(exemplar, with_neighbor);
        assert_ne!(exemplar, mod_key);
        assert_ne!(exemplar, mod_node_addr);
        assert_ne!(exemplar, mod_earning_wallet);
        assert_ne!(exemplar, mod_rate_pack);
        assert_ne!(exemplar, mod_is_bootstrap);
        assert_ne!(exemplar, mod_signed_gossip);
        assert_ne!(exemplar, mod_signature);
        assert_ne!(exemplar, mod_version);
    }

    #[test]
    fn increment_version_increments_node_record_version_by_1() {
        let mut this_node = make_node_record(123, true, false);

        assert_eq!(this_node.version(), 0);

        this_node.increment_version();
        assert_eq!(this_node.version(), 1);

        this_node.increment_version();
        assert_eq!(this_node.version(), 2);

        this_node.increment_version();
        assert_eq!(this_node.version(), 3);
    }

    #[test]
    fn set_version_sets_the_version() {
        let mut this_node = make_node_record(123, true, false);
        assert_eq!(this_node.version(), 0);

        this_node.set_version(10000);

        assert_eq!(this_node.version(), 10000);
    }

    #[test]
    fn set_earning_wallet_returns_true_when_the_earning_wallet_changes() {
        let mut this_node = make_node_record(1234, true, false);
        assert_eq!(this_node.earning_wallet(), Wallet::new("0x1234"));

        assert!(this_node.set_earning_wallet(Wallet::new("0x2345")));

        assert_eq!(this_node.earning_wallet(), Wallet::new("0x2345"));
    }

    #[test]
    fn set_earning_wallet_returns_false_when_the_wallet_does_not_change() {
        let mut this_node = make_node_record(1234, true, false);
        assert_eq!(this_node.earning_wallet(), Wallet::new("0x1234"));

        assert!(!this_node.set_earning_wallet(Wallet::new("0x1234")));

        assert_eq!(this_node.earning_wallet(), Wallet::new("0x1234"));
    }

    #[test]
    fn is_bootstrap_node_and_is_not_bootstrap_node_are_opposites() {
        let bootstrap = make_node_record(1234, true, true);
        let standard = make_node_record(2345, true, false);

        assert!(bootstrap.is_bootstrap_node());
        assert!(!bootstrap.is_not_bootstrap_node());
        assert!(!standard.is_bootstrap_node());
        assert!(standard.is_not_bootstrap_node());
    }

    #[test]
    fn set_desirable_when_no_change_from_default() {
        let mut this_node = make_node_record(5432, true, false);

        assert!(
            this_node.is_desirable(),
            "initial state should have been desirable"
        );
        this_node.set_desirable(true);
        assert!(
            this_node.is_desirable(),
            "Should be desirable after being set to true."
        );
    }

    #[test]
    fn set_desirable_to_false() {
        let mut this_node = make_node_record(5432, true, false);

        assert!(
            this_node.is_desirable(),
            "initial state should have been desirable"
        );
        this_node.set_desirable(false);
        assert!(
            !this_node.is_desirable(),
            "Should be undesirable after being set to false."
        );
    }

    #[test]
    fn from_gnr_to_nri_when_gossip_is_corrupt() {
        let corrupt_gnr = GossipNodeRecord {
            signed_data: PlainData::new(&[1, 2, 3, 4]),
            signature: CryptData::new(&[]),
            node_addr_opt: None,
        };

        let result = NodeRecordInner::try_from(corrupt_gnr);

        assert_eq! (Err(String::from ("ErrorImpl { code: Message(\"invalid type: integer `1`, expected struct NodeRecordInner\"), offset: 0 }")), result);
    }

    #[test]
    fn regenerate_signed_data_regenerates_signed_gossip_and_resigns() {
        let mut subject = make_node_record(1234, true, false);
        let cryptde = CryptDENull::from(subject.public_key());
        let initial_signed_gossip = subject.signed_gossip().clone();
        subject.increment_version();

        subject.regenerate_signed_gossip(&cryptde);

        let final_signed_gossip = subject.signed_gossip().clone();
        let final_signature = subject.signature().clone();
        assert_ne!(initial_signed_gossip, final_signed_gossip);
        assert_eq!(
            true,
            cryptde.verify_signature(&final_signed_gossip, &final_signature, cryptde.public_key())
        );
        let final_serialized = serde_cbor::ser::to_vec(&subject.inner).unwrap();
        assert_eq!(&final_serialized[..], final_signed_gossip.as_slice());
    }
}
