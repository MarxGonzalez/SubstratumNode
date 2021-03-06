// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

use futures::future::{err, ok};
use futures::{future, Future};
use web3::contract::{Contract, Options};
use web3::transports::{EventLoopHandle, Http};
use web3::types::{Address, BlockNumber, FilterBuilder, Log, H160, H256, U256};
use web3::Web3;

use crate::sub_lib::wallet::Wallet;

// HOT (Ropsten)
pub const TESTNET_CONTRACT_ADDRESS: Address = Address {
    0: [
        0xcd, 0x6c, 0x58, 0x8e, 0x00, 0x50, 0x32, 0xdd, 0x88, 0x2c, 0xd4, 0x3b, 0xf5, 0x3a, 0x32,
        0x12, 0x9b, 0xe8, 0x13, 0x02,
    ],
};

pub const CONTRACT_ABI: &str = r#"[{"constant":true,"inputs":[{"name":"owner","type":"address"}],"name":"balanceOf","outputs":[{"name":"","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":false,"inputs":[{"name":"to","type":"address"},{"name":"value","type":"uint256"}],"name":"transfer","outputs":[{"name":"","type":"bool"}],"payable":false,"stateMutability":"nonpayable","type":"function"}]"#;

const TRANSACTION_LITERAL: H256 = H256 {
    0: [
        0xdd, 0xf2, 0x52, 0xad, 0x1b, 0xe2, 0xc8, 0x9b, 0x69, 0xc2, 0xb0, 0x68, 0xfc, 0x37, 0x8d,
        0xaa, 0x95, 0x2b, 0xa7, 0xf1, 0x63, 0xc4, 0xa1, 0x16, 0x28, 0xf5, 0x5a, 0x4d, 0xf5, 0x23,
        0xb3, 0xef,
    ],
};

fn remove_0x(s: &str) -> &str {
    if s.starts_with("0x") {
        &s[2..]
    } else {
        s
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Transaction {
    pub block_number: U256,
    pub from: Wallet,
    pub amount: U256,
}

#[derive(Debug, PartialEq)]
pub enum BlockchainError {
    InvalidUrl,
    InvalidAddress,
    InvalidResponse,
    QueryFailed,
}

type BlockchainResult<T> = Box<dyn Future<Item = T, Error = BlockchainError> + Send>;
pub type Balance = BlockchainResult<U256>;
pub type Transactions = BlockchainResult<Vec<Transaction>>;

pub trait BlockchainInterface {
    fn retrieve_transactions(&self, start_block: u64, recipient: &Wallet) -> Transactions;

    fn get_eth_balance(&self, address: &Wallet) -> Balance;

    fn get_token_balance(&self, address: &Wallet) -> Balance;

    fn get_balances(&self, address: &Wallet) -> (Balance, Balance) {
        (
            self.get_eth_balance(address),
            self.get_token_balance(address),
        )
    }
}

pub struct BlockchainInterfaceClandestine {}

impl BlockchainInterface for BlockchainInterfaceClandestine {
    fn retrieve_transactions(&self, _start_block: u64, _recipient: &Wallet) -> Transactions {
        Box::new(ok(vec![]))
    }

    fn get_eth_balance(&self, _address: &Wallet) -> Balance {
        unimplemented!()
    }

    fn get_token_balance(&self, _address: &Wallet) -> Balance {
        unimplemented!()
    }
}

#[derive(Debug)]
pub struct BlockchainInterfaceRpc {
    contract_address: Address,
    // This must not be dropped for Web3 requests to be completed
    _event_loop_handle: EventLoopHandle,
    web3: Web3<Http>,
    contract: Contract<Http>,
}

impl BlockchainInterface for BlockchainInterfaceRpc {
    fn retrieve_transactions(&self, start_block: u64, recipient: &Wallet) -> Transactions {
        let to_address = match remove_0x(&recipient.address).parse::<H160>() {
            Ok(x) => x.into(),
            Err(_) => return Box::new(err(BlockchainError::InvalidAddress)),
        };

        let filter = FilterBuilder::default()
            .address(vec![self.contract_address])
            .from_block(BlockNumber::Number(start_block))
            .to_block(BlockNumber::Latest)
            .topics(
                Some(vec![TRANSACTION_LITERAL]),
                None,
                Some(vec![to_address]),
                None,
            )
            .build();

        let log_request = self.web3.eth().logs(filter);
        Box::new(log_request.then(|logs| {
            future::result::<Vec<Transaction>, BlockchainError>(match logs {
                Ok(logs) => {
                    if logs
                        .iter()
                        .any(|log| log.topics.len() < 2 || log.data.0.len() > 32)
                    {
                        Err(BlockchainError::InvalidResponse)
                    } else {
                        Ok(logs
                            .iter()
                            .filter_map(|log: &Log| match log.block_number {
                                Some(block_number) => Some(Transaction {
                                    block_number,
                                    from: Wallet::from(log.topics[1]),
                                    amount: U256::from(log.data.0.as_slice()),
                                }),
                                None => None,
                            })
                            .collect())
                    }
                }
                Err(_) => Err(BlockchainError::QueryFailed),
            })
        }))
    }

    fn get_eth_balance(&self, address: &Wallet) -> Balance {
        match remove_0x(&address.address).parse() {
            Ok(address) => Box::new(
                self.web3
                    .eth()
                    .balance(address, None)
                    .map_err(|_| BlockchainError::QueryFailed),
            ),
            Err(_) => Box::new(err(BlockchainError::InvalidAddress)),
        }
    }

    fn get_token_balance(&self, address: &Wallet) -> Balance {
        match remove_0x(&address.address).parse::<Address>() {
            Ok(address) => Box::new(
                self.contract
                    .query("balanceOf", address, None, Options::with(|_| {}), None)
                    .map_err(|_| BlockchainError::QueryFailed),
            ),
            Err(_) => Box::new(err(BlockchainError::InvalidAddress)),
        }
    }
}

impl BlockchainInterfaceRpc {
    pub fn new(
        blockchain_service_url: String,
        contract_address: Address,
    ) -> Result<BlockchainInterfaceRpc, BlockchainError> {
        match Http::new(&blockchain_service_url) {
            Ok((_loop, http)) => {
                let web3 = Web3::new(http);
                let contract =
                    Contract::from_json(web3.eth(), contract_address, CONTRACT_ABI.as_bytes())
                        .expect("Unable to initialize contract.");
                Ok(BlockchainInterfaceRpc {
                    contract_address,
                    _event_loop_handle: _loop,
                    web3,
                    contract,
                })
            }
            Err(_) => Err(BlockchainError::InvalidUrl),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sub_lib::wallet::Wallet;
    use crate::test_utils::test_utils::find_free_port;
    use serde_json::Value;
    use simple_server::Server;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn blockchain_interface_rpc_retrieves_transactions() {
        let to = "0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc";
        let port = find_free_port();

        let (tx, rx) = mpsc::sync_channel(1337);
        thread::spawn(move || {
            Server::new(move |req, mut rsp| {
                tx.send(req.body().clone()).unwrap();
                Ok(rsp.body(r#"{"jsonrpc":"2.0","id":3,"result":[{"address":"0xcd6c588e005032dd882cd43bf53a32129be81302","blockHash":"0x1a24b9169cbaec3f6effa1f600b70c7ab9e8e86db44062b49132a4415d26732a","blockNumber":"0x4be663","data":"0x0000000000000000000000000000000000000000000000000010000000000000","logIndex":"0x0","removed":false,"topics":["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef","0x0000000000000000000000003f69f9efd4f2592fd70be8c32ecd9dce71c472fc","0x000000000000000000000000adc1853c7859369639eb414b6342b36288fe6092"],"transactionHash":"0x955cec6ac4f832911ab894ce16aa22c3003f46deff3f7165b32700d2f5ff0681","transactionIndex":"0x0"}]}"#.as_bytes().to_vec())?)
            }).listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .retrieve_transactions(
                42,
                &Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"),
            )
            .wait()
            .unwrap();

        let body: Value = serde_json::de::from_slice(&rx.recv().unwrap()).unwrap();
        assert_eq!(
            format!("\"0x000000000000000000000000{}\"", &to[2..]),
            body["params"][0]["topics"][2].to_string(),
        );
        assert_eq!(
            vec![Transaction {
                block_number: U256::from(4_974_179),
                from: Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"),
                amount: U256::from(4_503_599_627_370_496u64),
            }],
            result,
        )
    }

    #[test]
    fn blockchain_interface_rpc_returns_an_error_if_the_blockchain_service_url_is_invalid() {
        let subject_result =
            BlockchainInterfaceRpc::new("http://λ:8545".to_string(), TESTNET_CONTRACT_ADDRESS);

        assert_eq!(
            BlockchainError::InvalidUrl,
            subject_result.expect_err("Expected an Err, got Ok")
        );
    }

    #[test]
    fn blockchain_interface_rpc_returns_an_error_if_the_to_address_is_invalid() {
        let subject = BlockchainInterfaceRpc::new(
            "http://127.0.0.1:8545".to_string(),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .retrieve_transactions(42, &Wallet::new("0x3f69f9efd4f2592fd70beecd9dce71c472fc"))
            .wait();

        assert_eq!(
            BlockchainError::InvalidAddress,
            result.expect_err("Expected an Err, got Ok")
        );
    }

    #[test]
    fn blockchain_interface_rpc_returns_an_error_if_a_response_with_too_few_topics_is_returned() {
        let port = find_free_port();

        thread::spawn(move || {
            Server::new(|_req, mut rsp| {
                Ok(rsp.body(r#"{"jsonrpc":"2.0","id":3,"result":[{"address":"0xcd6c588e005032dd882cd43bf53a32129be81302","blockHash":"0x1a24b9169cbaec3f6effa1f600b70c7ab9e8e86db44062b49132a4415d26732a","blockNumber":"0x4be663","data":"0x0000000000000000000000000000000000000000000000056bc75e2d63100000","logIndex":"0x0","removed":false,"topics":["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"],"transactionHash":"0x955cec6ac4f832911ab894ce16aa22c3003f46deff3f7165b32700d2f5ff0681","transactionIndex":"0x0"}]}"#.as_bytes().to_vec())?)
            })
                .listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .retrieve_transactions(
                42,
                &Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"),
            )
            .wait();

        assert_eq!(
            BlockchainError::InvalidResponse,
            result.expect_err("Expected an Err, got Ok")
        );
    }

    #[test]
    fn blockchain_interface_rpc_returns_an_error_if_a_response_with_data_that_is_too_long_is_returned(
    ) {
        let port = find_free_port();

        thread::spawn(move || {
            Server::new(move |_req, mut rsp| {
                Ok(rsp.body(r#"{"jsonrpc":"2.0","id":3,"result":[{"address":"0xcd6c588e005032dd882cd43bf53a32129be81302","blockHash":"0x1a24b9169cbaec3f6effa1f600b70c7ab9e8e86db44062b49132a4415d26732a","blockNumber":"0x4be663","data":"0x0000000000000000000000000000000000000000000000056bc75e2d6310000001","logIndex":"0x0","removed":false,"topics":["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef","0x0000000000000000000000003f69f9efd4f2592fd70be8c32ecd9dce71c472fc","0x000000000000000000000000adc1853c7859369639eb414b6342b36288fe6092"],"transactionHash":"0x955cec6ac4f832911ab894ce16aa22c3003f46deff3f7165b32700d2f5ff0681","transactionIndex":"0x0"}]}"#.as_bytes().to_vec())?)
            }).listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .retrieve_transactions(
                42,
                &Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"),
            )
            .wait();

        assert_eq!(Err(BlockchainError::InvalidResponse), result);
    }

    #[test]
    fn blockchain_interface_rpc_ignores_transaction_logs_that_have_no_block_number() {
        let port = find_free_port();

        thread::spawn(move || {
            Server::new(|_req, mut rsp| {
                Ok(rsp.body(r#"{"jsonrpc":"2.0","id":3,"result":[{"address":"0xcd6c588e005032dd882cd43bf53a32129be81302","blockHash":"0x1a24b9169cbaec3f6effa1f600b70c7ab9e8e86db44062b49132a4415d26732a","data":"0x0000000000000000000000000000000000000000000000000010000000000000","logIndex":"0x0","removed":false,"topics":["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef","0x0000000000000000000000003f69f9efd4f2592fd70be8c32ecd9dce71c472fc","0x000000000000000000000000adc1853c7859369639eb414b6342b36288fe6092"],"transactionHash":"0x955cec6ac4f832911ab894ce16aa22c3003f46deff3f7165b32700d2f5ff0681","transactionIndex":"0x0"}]}"#.as_bytes().to_vec())?)
            })
                .listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .retrieve_transactions(
                42,
                &Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"),
            )
            .wait();

        assert_eq!(Ok(vec![]), result);
    }

    #[test]
    fn blockchain_interface_rpc_can_retrieve_eth_balance_of_a_wallet() {
        let port = find_free_port();

        thread::spawn(move || {
            Server::new(|_req, mut rsp| {
                Ok(rsp.body(r#"{"jsonrpc":"2.0","id":0,"result":"0xFFFF"}"#.as_bytes().to_vec())?)
            })
            .listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .get_eth_balance(&Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"))
            .wait();

        assert_eq!(U256::from(65535), result.unwrap());
    }

    #[test]
    fn blockchain_interface_rpc_returns_an_error_when_requesting_eth_balance_of_an_invalid_wallet()
    {
        let subject = BlockchainInterfaceRpc::new(
            String::from("http://127.0.0.1:8545"),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .get_eth_balance(&Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fQ"))
            .wait();

        assert_eq!(Err(BlockchainError::InvalidAddress), result);
    }

    #[test]
    fn blockchain_interface_rpc_returns_an_error_for_unintelligible_response_to_requesting_eth_balance(
    ) {
        let port = find_free_port();

        thread::spawn(move || {
            Server::new(|_req, mut rsp| {
                Ok(rsp.body(r#"{"jsonrpc":"2.0","id":0,"result":"0xFFFQ"}"#.as_bytes().to_vec())?)
            })
            .listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .get_eth_balance(&Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"))
            .wait();

        assert_eq!(Err(BlockchainError::QueryFailed), result);
    }

    #[test]
    fn blockchain_interface_rpc_can_retrieve_token_balance_of_a_wallet() {
        let port = find_free_port();

        thread::spawn(move || {
            Server::new(|_req, mut rsp| {
                Ok(rsp.body(
                    r#"{"jsonrpc":"2.0","id":0,"result":"0x000000000000000000000000000000000000000000000000000000000000FFFF"}"#
                        .as_bytes()
                        .to_vec(),
                )?)
            })
                .listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .get_token_balance(&Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"))
            .wait();

        assert_eq!(U256::from(65535), result.unwrap());
    }

    #[test]
    fn blockchain_interface_rpc_returns_an_error_when_requesting_token_balance_of_an_invalid_wallet(
    ) {
        let subject = BlockchainInterfaceRpc::new(
            String::from("http://127.0.0.1:8545"),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .get_token_balance(&Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fQ"))
            .wait();

        assert_eq!(Err(BlockchainError::InvalidAddress), result);
    }

    #[test]
    fn blockchain_interface_rpc_returns_an_error_for_unintelligible_response_when_requesting_token_balance(
    ) {
        let port = find_free_port();

        thread::spawn(move || {
            Server::new(|_req, mut rsp| {
                Ok(rsp.body(
                    r#"{"jsonrpc":"2.0","id":0,"result":"0x000000000000000000000000000000000000000000000000000000000000FFFQ"}"#
                        .as_bytes()
                        .to_vec(),
                )?)
            })
            .listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let result = subject
            .get_token_balance(&Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"))
            .wait();

        assert_eq!(Err(BlockchainError::QueryFailed), result);
    }

    #[test]
    fn blockchain_interface_rpc_can_request_both_eth_and_token_balances_happy_path() {
        let port = find_free_port();

        thread::spawn(move || {
            Server::new(|_req, mut rsp| {
                Ok(rsp.body(
                    r#"{"jsonrpc":"2.0","id":0,"result":"0x0000000000000000000000000000000000000000000000000000000000000001"}"#
                        .as_bytes()
                        .to_vec(),
                )?)
            })
            .listen("127.0.0.1", &format!("{}", port));
        });

        let subject = BlockchainInterfaceRpc::new(
            format!("http://127.0.0.1:{}", port),
            TESTNET_CONTRACT_ADDRESS,
        )
        .unwrap();

        let results: (Balance, Balance) =
            subject.get_balances(&Wallet::new("0x3f69f9efd4f2592fd70be8c32ecd9dce71c472fc"));
        let eth_balance = results.0.wait().unwrap();
        let token_balance = results.1.wait().unwrap();

        assert_eq!(U256::from(1), eth_balance);
        assert_eq!(U256::from(1), token_balance)
    }
}
