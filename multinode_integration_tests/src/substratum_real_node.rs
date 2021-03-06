// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.
use crate::command::Command;
use crate::substratum_client::SubstratumNodeClient;
use crate::substratum_node::NodeReference;
use crate::substratum_node::PortSelector;
use crate::substratum_node::SubstratumNode;
use crate::substratum_node::SubstratumNodeUtils;
use node_lib::sub_lib::accountant;
use node_lib::sub_lib::accountant::TEMPORARY_CONSUMING_WALLET;
use node_lib::sub_lib::cryptde::PublicKey;
use node_lib::sub_lib::cryptde_null::CryptDENull;
use node_lib::sub_lib::neighborhood::sentinel_ip_addr;
use node_lib::sub_lib::neighborhood::RatePack;
use node_lib::sub_lib::neighborhood::DEFAULT_RATE_PACK;
use node_lib::sub_lib::neighborhood::ZERO_RATE_PACK;
use node_lib::sub_lib::node_addr::NodeAddr;
use node_lib::sub_lib::wallet::Wallet;
use regex::Regex;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::rc::Rc;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct Firewall {
    ports_to_open: Vec<u16>,
}

#[derive(PartialEq, Clone, Debug, Copy)]
pub enum NodeType {
    Standard,
    Bootstrap,
}

#[derive(PartialEq, Clone, Debug, Copy)]
pub enum LocalIpInfo {
    ZeroHop,
    DistributedUnknown,
    DistributedKnown(IpAddr),
}

#[derive(PartialEq, Clone)]
pub struct NodeStartupConfig {
    pub ip_info: LocalIpInfo,
    pub dns_servers: Vec<IpAddr>,
    pub neighbors: Vec<NodeReference>,
    pub node_type: NodeType,
    pub clandestine_port_opt: Option<u16>,
    pub dns_target: IpAddr,
    pub dns_port: u16,
    pub earning_wallet: Wallet,
    pub rate_pack: RatePack,
    pub consuming_private_key: Option<String>,
    pub firewall: Option<Firewall>,
}

impl NodeStartupConfig {
    pub fn new() -> NodeStartupConfig {
        NodeStartupConfig {
            ip_info: LocalIpInfo::ZeroHop,
            dns_servers: Vec::new(),
            neighbors: Vec::new(),
            node_type: NodeType::Bootstrap,
            clandestine_port_opt: None,
            dns_target: sentinel_ip_addr(),
            dns_port: 0,
            earning_wallet: accountant::DEFAULT_EARNING_WALLET.clone(),
            rate_pack: DEFAULT_RATE_PACK,
            consuming_private_key: Some(String::from(
                "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
            )),
            firewall: None,
        }
    }

    pub fn firewall(&self) -> Option<Firewall> {
        self.firewall.clone()
    }

    fn make_args(&self) -> Vec<String> {
        let mut args = vec![];
        if let LocalIpInfo::DistributedKnown(ip_addr) = self.ip_info {
            args.push("--ip".to_string());
            args.push(format!("{}", ip_addr));
        }
        args.push("--dns_servers".to_string());
        args.push(Self::join_ip_addrs(&self.dns_servers));
        self.neighbors.iter().for_each(|neighbor| {
            args.push("--neighbor".to_string());
            args.push(format!("{}", neighbor));
        });
        args.push("--wallet_address".to_string());
        args.push(format!("{}", self.earning_wallet.address));
        args.push("--node_type".to_string());
        args.push(
            match self.node_type {
                NodeType::Standard => "standard",
                NodeType::Bootstrap => "bootstrap",
            }
            .to_string(),
        );
        if let Some(clandestine_port) = self.clandestine_port_opt {
            args.push("--clandestine_port".to_string());
            args.push(format!("{}", clandestine_port));
        }
        args.push("--log_level".to_string());
        args.push("trace".to_string());
        args.push("--data_directory".to_string());
        args.push("/node_root/home".to_string());
        args
    }

    fn join_ip_addrs(ip_addrs: &Vec<IpAddr>) -> String {
        ip_addrs
            .iter()
            .map(|ip_addr| format!("{}", ip_addr))
            .collect::<Vec<String>>()
            .join(",")
    }
}

pub struct NodeStartupConfigBuilder {
    ip_info: LocalIpInfo,
    dns_servers: Vec<IpAddr>,
    neighbors: Vec<NodeReference>,
    node_type: NodeType,
    clandestine_port_opt: Option<u16>,
    dns_target: IpAddr,
    dns_port: u16,
    earning_wallet: Wallet,
    rate_pack: RatePack,
    consuming_private_key: Option<String>,
    firewall: Option<Firewall>,
}

impl NodeStartupConfigBuilder {
    pub fn zero_hop() -> NodeStartupConfigBuilder {
        NodeStartupConfigBuilder {
            ip_info: LocalIpInfo::ZeroHop,
            dns_servers: vec![IpAddr::from_str("8.8.8.8").unwrap()],
            neighbors: vec![],
            node_type: NodeType::Standard,
            clandestine_port_opt: None,
            dns_target: IpAddr::from_str("127.0.0.1").unwrap(),
            dns_port: 53,
            earning_wallet: accountant::DEFAULT_EARNING_WALLET.clone(),
            rate_pack: ZERO_RATE_PACK.clone(),
            consuming_private_key: None,
            firewall: None,
        }
    }

    pub fn standard() -> NodeStartupConfigBuilder {
        NodeStartupConfigBuilder {
            ip_info: LocalIpInfo::DistributedUnknown,
            dns_servers: vec![IpAddr::from_str("8.8.8.8").unwrap()],
            neighbors: vec![],
            node_type: NodeType::Standard,
            clandestine_port_opt: None,
            dns_target: IpAddr::from_str("127.0.0.1").unwrap(),
            dns_port: 53,
            earning_wallet: accountant::DEFAULT_EARNING_WALLET.clone(),
            rate_pack: DEFAULT_RATE_PACK.clone(),
            consuming_private_key: Some(String::from(
                "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
            )),
            firewall: None,
        }
    }

    pub fn bootstrap() -> NodeStartupConfigBuilder {
        NodeStartupConfigBuilder {
            ip_info: LocalIpInfo::DistributedUnknown,
            dns_servers: vec![IpAddr::from_str("8.8.8.8").unwrap()],
            neighbors: vec![],
            node_type: NodeType::Bootstrap,
            clandestine_port_opt: None,
            dns_target: IpAddr::from_str("127.0.0.1").unwrap(),
            dns_port: 53,
            earning_wallet: accountant::DEFAULT_EARNING_WALLET.clone(),
            rate_pack: ZERO_RATE_PACK,
            consuming_private_key: None,
            firewall: None,
        }
    }

    pub fn copy(config: &NodeStartupConfig) -> NodeStartupConfigBuilder {
        NodeStartupConfigBuilder {
            ip_info: config.ip_info.clone(),
            dns_servers: config.dns_servers.clone(),
            neighbors: config.neighbors.clone(),
            node_type: config.node_type,
            clandestine_port_opt: config.clandestine_port_opt,
            dns_target: config.dns_target.clone(),
            dns_port: config.dns_port,
            earning_wallet: config.earning_wallet.clone(),
            rate_pack: config.rate_pack.clone(),
            consuming_private_key: config.consuming_private_key.clone(),
            firewall: config.firewall.clone(),
        }
    }

    pub fn ip(mut self, value: IpAddr) -> NodeStartupConfigBuilder {
        self.ip_info = LocalIpInfo::DistributedKnown(value);
        self
    }

    pub fn dns_servers(mut self, value: Vec<IpAddr>) -> NodeStartupConfigBuilder {
        self.dns_servers = value;
        self
    }

    pub fn neighbor(mut self, value: NodeReference) -> NodeStartupConfigBuilder {
        self.neighbors.push(value);
        self
    }

    pub fn neighbors(mut self, value: Vec<NodeReference>) -> NodeStartupConfigBuilder {
        self.neighbors = value;
        self
    }

    pub fn node_type(mut self, value: NodeType) -> NodeStartupConfigBuilder {
        self.node_type = value;
        self
    }

    pub fn clandestine_port(mut self, value: u16) -> NodeStartupConfigBuilder {
        self.clandestine_port_opt = Some(value);
        self
    }

    pub fn dns_target(mut self, value: IpAddr) -> NodeStartupConfigBuilder {
        self.dns_target = value;
        self
    }

    pub fn dns_port(mut self, value: u16) -> NodeStartupConfigBuilder {
        self.dns_port = value;
        self
    }

    pub fn earning_wallet(mut self, value: Wallet) -> NodeStartupConfigBuilder {
        self.earning_wallet = value;
        self
    }

    pub fn rate_pack(mut self, value: RatePack) -> NodeStartupConfigBuilder {
        self.rate_pack = value;
        self
    }

    pub fn consuming_private_key(mut self, value: &str) -> NodeStartupConfigBuilder {
        self.consuming_private_key = Some(String::from(value));
        self
    }

    pub fn open_firewall_port(mut self, port: u16) -> NodeStartupConfigBuilder {
        if self.firewall.is_none() {
            self.firewall = Some(Firewall {
                ports_to_open: vec![],
            })
        }
        self.firewall
            .as_mut()
            .expect("Firewall magically disappeared")
            .ports_to_open
            .push(port);
        self
    }

    pub fn build(self) -> NodeStartupConfig {
        NodeStartupConfig {
            ip_info: self.ip_info,
            dns_servers: self.dns_servers,
            neighbors: self.neighbors,
            node_type: self.node_type,
            clandestine_port_opt: self.clandestine_port_opt,
            dns_target: self.dns_target,
            dns_port: self.dns_port,
            earning_wallet: self.earning_wallet,
            rate_pack: self.rate_pack,
            consuming_private_key: self.consuming_private_key,
            firewall: self.firewall,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SubstratumRealNode {
    guts: Rc<SubstratumRealNodeGuts>,
}

impl SubstratumNode for SubstratumRealNode {
    fn name(&self) -> &str {
        &self.guts.name
    }

    fn node_reference(&self) -> NodeReference {
        self.guts.node_reference.clone()
    }

    fn public_key(&self) -> &PublicKey {
        &self.guts.node_reference.public_key
    }

    fn cryptde(&self) -> CryptDENull {
        CryptDENull::from(&self.public_key())
    }

    fn ip_address(&self) -> IpAddr {
        self.guts.container_ip
    }

    fn port_list(&self) -> Vec<u16> {
        self.node_reference().node_addr.ports().clone()
    }

    fn node_addr(&self) -> NodeAddr {
        NodeAddr::new(&self.ip_address(), &self.port_list())
    }

    fn socket_addr(&self, port_selector: PortSelector) -> SocketAddr {
        SubstratumNodeUtils::socket_addr(&self.node_addr(), port_selector, self.name())
    }

    fn earning_wallet(&self) -> Wallet {
        self.guts.earning_wallet.clone()
    }

    fn consuming_wallet(&self) -> Option<Wallet> {
        self.guts.consuming_wallet.clone()
    }

    fn rate_pack(&self) -> RatePack {
        self.guts.rate_pack.clone()
    }

    fn make_client(&self, port: u16) -> SubstratumNodeClient {
        let socket_addr = SocketAddr::new(self.ip_address(), port);
        SubstratumNodeClient::new(socket_addr)
    }
}

impl SubstratumRealNode {
    pub fn start(
        startup_config: NodeStartupConfig,
        index: usize,
        host_node_parent_dir: Option<String>,
    ) -> SubstratumRealNode {
        let ip_addr = IpAddr::V4(Ipv4Addr::new(172, 18, 1, index as u8));
        let name = format!("test_node_{}", index);
        let earning_wallet = startup_config.earning_wallet.clone();
        let rate_pack = startup_config.rate_pack.clone();
        SubstratumNodeUtils::clean_up_existing_container(&name[..]);
        let real_startup_config = match startup_config.ip_info {
            LocalIpInfo::ZeroHop => startup_config.clone(),
            LocalIpInfo::DistributedUnknown => NodeStartupConfigBuilder::copy(&startup_config)
                .ip(ip_addr)
                .build(),
            LocalIpInfo::DistributedKnown(ip_addr) => panic!(
                "Can't pre-specify the IP address of a SubstratumRealNode: {}",
                ip_addr
            ),
        };
        let root_dir = match host_node_parent_dir {
            Some(dir) => dir,
            None => SubstratumNodeUtils::find_project_root(),
        };
        Self::do_docker_run(&real_startup_config, &root_dir, ip_addr, &name).unwrap();

        Self::exec_command_on_container_and_detach(
            &name,
            vec!["/usr/local/bin/port_exposer", "80:8080", "443:8443"],
        )
        .expect("port_exposer wouldn't run");
        match &real_startup_config.firewall {
            None => (),
            Some(firewall) => {
                Self::create_impenetrable_firewall(&name);
                firewall.ports_to_open.iter().for_each(|port| {
                    Self::open_firewall_port(&name, *port)
                        .expect(&format!("Can't open port {}", *port))
                });
            }
        }
        let node_args = real_startup_config.make_args();
        let mut command_parts = vec!["/node_root/node/SubstratumNode"];
        command_parts.extend(node_args.iter().map(|s| s.as_str()));
        Self::exec_command_on_container_and_detach(&name, command_parts)
            .expect("Couldn't start SubstratumNode");

        let node_reference = SubstratumRealNode::extract_node_reference(&name).unwrap();
        let guts = Rc::new(SubstratumRealNodeGuts {
            name,
            container_ip: ip_addr,
            node_reference,
            earning_wallet,
            consuming_wallet: Some(TEMPORARY_CONSUMING_WALLET.clone()),
            rate_pack,
            root_dir,
        });
        SubstratumRealNode { guts }
    }

    pub fn root_dir(&self) -> String {
        self.guts.root_dir.clone()
    }

    pub fn node_home_dir(root_dir: &String, name: &String) -> String {
        format!("{}/generated/node_homes/{}", root_dir, name)
    }

    pub fn home_dir(&self) -> String {
        Self::node_home_dir(&self.root_dir(), &String::from(self.name()))
    }

    pub fn open_firewall_port(name: &str, port: u16) -> Result<(), ()> {
        let port_str = format!("{}", port);
        match Self::exec_command_on_container_and_wait(
            name,
            vec![
                "iptables", "-A", "INPUT", "-p", "tcp", "--dport", &port_str, "-j", "ACCEPT",
            ],
        ) {
            Err(_) => Err(()),
            Ok(_) => Ok(()),
        }
    }

    fn do_docker_run(
        startup_config: &NodeStartupConfig,
        root_dir: &String,
        ip_addr: IpAddr,
        container_name_ref: &String,
    ) -> Result<(), String> {
        let container_name = container_name_ref.clone();
        let node_command_dir = format!("{}/node/target/release", root_dir);
        let host_node_home_dir = Self::node_home_dir(root_dir, container_name_ref);
        let test_runner_node_home_dir = Self::node_home_dir(
            &SubstratumNodeUtils::find_project_root(),
            container_name_ref,
        );
        Command::new(
            "rm",
            Command::strings(vec!["-r", test_runner_node_home_dir.as_str()]),
        )
        .wait_for_exit();
        match Command::new(
            "mkdir",
            Command::strings(vec!["-p", test_runner_node_home_dir.as_str()]),
        )
        .wait_for_exit()
        {
            0 => (),
            _ => panic!(
                "Couldn't create home directory for node {} at {}",
                container_name, test_runner_node_home_dir
            ),
        }
        match Command::new(
            "chmod",
            Command::strings(vec!["777", test_runner_node_home_dir.as_str()]),
        )
        .wait_for_exit()
        {
            0 => (),
            _ => panic!(
                "Couldn't chmod 777 home directory for node {} at {}",
                container_name, test_runner_node_home_dir
            ),
        }
        let ip_addr_string = format!("{}", ip_addr);
        let node_binary_v_param = format!("{}:/node_root/node", node_command_dir);
        let home_v_param = format!("{}:/node_root/home", host_node_home_dir);

        let mut args = vec![
            "run",
            "--detach",
            "--ip",
            ip_addr_string.as_str(),
            "--dns",
            "127.0.0.1",
            "--name",
            container_name.as_str(),
            "--net",
            "integration_net",
            "-v",
            node_binary_v_param.as_str(),
            "-v",
            home_v_param.as_str(),
            "-e",
            "RUST_BACKTRACE=full",
            "--cap-add=NET_ADMIN",
        ];

        let maybe_key = match startup_config.consuming_private_key.clone() {
            Some(key) => format!("CONSUMING_PRIVATE_KEY={}", key),
            None => "".to_string(),
        };
        if startup_config.consuming_private_key.is_some() {
            args.push("-e");
            args.push(&maybe_key)
        }
        args.push("test_node_image");
        let mut command = Command::new("docker", Command::strings(args));
        command.stdout_or_stderr()?;
        Ok(())
    }

    fn exec_command_on_container_and_detach(
        name: &str,
        command_parts: Vec<&str>,
    ) -> Result<String, String> {
        Self::do_docker_exec(name, command_parts, "-d")
    }

    fn exec_command_on_container_and_wait(
        name: &str,
        command_parts: Vec<&str>,
    ) -> Result<String, String> {
        Self::do_docker_exec(name, command_parts, "-t")
    }

    fn do_docker_exec(
        name: &str,
        command_parts: Vec<&str>,
        exec_type: &str,
    ) -> Result<String, String> {
        let mut params = vec!["exec", exec_type, name];
        params.extend(command_parts);
        let mut command = Command::new("docker", Command::strings(params));
        command.stdout_or_stderr()
    }

    fn create_impenetrable_firewall(name: &str) {
        Self::exec_command_on_container_and_wait(name, vec!["iptables", "-P", "INPUT", "DROP"])
            .expect("Can't completely reject all incoming data by default");
        Self::exec_command_on_container_and_wait(
            name,
            vec!["iptables", "-A", "INPUT", "-i", "lo", "-j", "ACCEPT"],
        )
        .expect("Can't add exception to allow incoming data from loopback interface");
        Self::exec_command_on_container_and_wait(
            name,
            vec![
                "iptables",
                "-A",
                "INPUT",
                "-m",
                "conntrack",
                "--ctstate",
                "RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ],
        )
        .expect("Can't add exception to allow input that is respondent to past output");
    }

    fn extract_node_reference(name: &String) -> Result<NodeReference, String> {
        let regex = Regex::new(r"SubstratumNode local descriptor: ([^:]+:[\d.]+:[\d,]*)").unwrap();
        let mut retries_left = 5;
        loop {
            thread::sleep(Duration::from_millis(100));
            println!("Checking for {} startup", name);
            let output = Self::exec_command_on_container_and_wait(
                name,
                vec!["cat", "/tmp/SubstratumNode.log"],
            )?;
            match regex.captures(output.as_str()) {
                Some(captures) => {
                    let node_reference =
                        NodeReference::from_str(captures.get(1).unwrap().as_str()).unwrap();
                    println!("{} startup detected at {}", name, node_reference);
                    return Ok(node_reference);
                }
                None => {
                    if retries_left <= 0 {
                        return Err(format!("Node {} never started", name));
                    } else {
                        retries_left -= 1;
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct SubstratumRealNodeGuts {
    name: String,
    container_ip: IpAddr,
    node_reference: NodeReference,
    earning_wallet: Wallet,
    consuming_wallet: Option<Wallet>,
    rate_pack: RatePack,
    root_dir: String,
}

impl Drop for SubstratumRealNodeGuts {
    fn drop(&mut self) {
        SubstratumNodeUtils::stop(self.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use node_lib::persistent_configuration::{HTTP_PORT, TLS_PORT};

    #[test]
    fn node_startup_config_builder_zero_hop() {
        let result = NodeStartupConfigBuilder::zero_hop().build();

        assert_eq!(result.ip_info, LocalIpInfo::ZeroHop);
        assert_eq!(
            result.dns_servers,
            vec!(IpAddr::from_str("8.8.8.8").unwrap())
        );
        assert_eq!(result.neighbors, vec!());
        assert_eq!(result.node_type, NodeType::Standard);
        assert_eq!(result.clandestine_port_opt, None);
        assert_eq!(result.dns_target, IpAddr::from_str("127.0.0.1").unwrap());
        assert_eq!(result.dns_port, 53);
    }

    #[test]
    fn node_startup_config_builder_standard() {
        let result = NodeStartupConfigBuilder::standard().build();

        assert_eq!(result.ip_info, LocalIpInfo::DistributedUnknown);
        assert_eq!(
            result.dns_servers,
            vec!(IpAddr::from_str("8.8.8.8").unwrap())
        );
        assert_eq!(result.neighbors, vec!());
        assert_eq!(result.node_type, NodeType::Standard);
        assert_eq!(result.clandestine_port_opt, None);
        assert_eq!(result.dns_target, IpAddr::from_str("127.0.0.1").unwrap());
        assert_eq!(result.dns_port, 53);
    }

    #[test]
    fn node_startup_config_builder_bootstrap() {
        let result = NodeStartupConfigBuilder::bootstrap().build();

        assert_eq!(result.ip_info, LocalIpInfo::DistributedUnknown);
        assert_eq!(
            result.dns_servers,
            vec!(IpAddr::from_str("8.8.8.8").unwrap())
        );
        assert_eq!(result.neighbors, vec!());
        assert_eq!(result.node_type, NodeType::Bootstrap);
        assert_eq!(result.clandestine_port_opt, None);
        assert_eq!(result.dns_target, IpAddr::from_str("127.0.0.1").unwrap());
        assert_eq!(result.dns_port, 53);
    }

    #[test]
    fn node_startup_config_builder_settings() {
        let ip_addr = IpAddr::from_str("1.2.3.4").unwrap();
        let one_neighbor_key = PublicKey::new(&[1, 2, 3, 4]);
        let one_neighbor_ip_addr = IpAddr::from_str("4.5.6.7").unwrap();
        let one_neighbor_ports = vec![1234, 2345];
        let another_neighbor_key = PublicKey::new(&[2, 3, 4, 5]);
        let another_neighbor_ip_addr = IpAddr::from_str("5.6.7.8").unwrap();
        let another_neighbor_ports = vec![3456, 4567];
        let dns_servers = vec![
            IpAddr::from_str("2.3.4.5").unwrap(),
            IpAddr::from_str("3.4.5.6").unwrap(),
        ];
        let neighbors = vec![
            NodeReference::new(
                one_neighbor_key.clone(),
                one_neighbor_ip_addr.clone(),
                one_neighbor_ports.clone(),
            ),
            NodeReference::new(
                another_neighbor_key.clone(),
                another_neighbor_ip_addr.clone(),
                another_neighbor_ports.clone(),
            ),
        ];
        let dns_target = IpAddr::from_str("8.9.10.11").unwrap();

        let result = NodeStartupConfigBuilder::bootstrap()
            .ip(ip_addr)
            .dns_servers(dns_servers.clone())
            .neighbor(neighbors[0].clone())
            .neighbor(neighbors[1].clone())
            .node_type(NodeType::Standard)
            .dns_target(dns_target)
            .dns_port(35)
            .build();

        assert_eq!(result.ip_info, LocalIpInfo::DistributedKnown(ip_addr));
        assert_eq!(result.dns_servers, dns_servers);
        assert_eq!(result.neighbors, neighbors);
        assert_eq!(result.node_type, NodeType::Standard);
        assert_eq!(result.clandestine_port_opt, None);
        assert_eq!(result.dns_target, dns_target);
        assert_eq!(result.dns_port, 35);
    }

    #[test]
    fn node_startup_config_builder_copy() {
        let original = NodeStartupConfig {
            ip_info: LocalIpInfo::DistributedUnknown,
            dns_servers: vec![IpAddr::from_str("255.255.255.255").unwrap()],
            neighbors: vec![NodeReference::new(
                PublicKey::new(&[255]),
                IpAddr::from_str("255.255.255.255").unwrap(),
                vec![255],
            )],
            node_type: NodeType::Standard,
            clandestine_port_opt: Some(1234),
            dns_target: IpAddr::from_str("255.255.255.255").unwrap(),
            dns_port: 54,
            earning_wallet: Wallet::new("booga"),
            rate_pack: RatePack {
                routing_byte_rate: 10,
                routing_service_rate: 20,
                exit_byte_rate: 30,
                exit_service_rate: 40,
            },
            consuming_private_key: Some(String::from(
                "ABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCD",
            )),
            firewall: Some(Firewall {
                ports_to_open: vec![HTTP_PORT, TLS_PORT],
            }),
        };
        let ip_addr = IpAddr::from_str("1.2.3.4").unwrap();
        let one_neighbor_key = PublicKey::new(&[1, 2, 3, 4]);
        let one_neighbor_ip_addr = IpAddr::from_str("4.5.6.7").unwrap();
        let one_neighbor_ports = vec![1234, 2345];
        let another_neighbor_key = PublicKey::new(&[2, 3, 4, 5]);
        let another_neighbor_ip_addr = IpAddr::from_str("5.6.7.8").unwrap();
        let another_neighbor_ports = vec![3456, 4567];
        let dns_servers = vec![
            IpAddr::from_str("2.3.4.5").unwrap(),
            IpAddr::from_str("3.4.5.6").unwrap(),
        ];
        let neighbors = vec![
            NodeReference::new(
                one_neighbor_key.clone(),
                one_neighbor_ip_addr.clone(),
                one_neighbor_ports.clone(),
            ),
            NodeReference::new(
                another_neighbor_key.clone(),
                another_neighbor_ip_addr.clone(),
                another_neighbor_ports.clone(),
            ),
        ];
        let dns_target = IpAddr::from_str("8.9.10.11").unwrap();

        let result = NodeStartupConfigBuilder::copy(&original)
            .ip(ip_addr)
            .dns_servers(dns_servers.clone())
            .neighbors(neighbors.clone())
            .node_type(NodeType::Bootstrap)
            .clandestine_port(1234)
            .dns_target(dns_target)
            .dns_port(35)
            .build();

        assert_eq!(result.ip_info, LocalIpInfo::DistributedKnown(ip_addr));
        assert_eq!(result.dns_servers, dns_servers);
        assert_eq!(result.neighbors, neighbors);
        assert_eq!(result.node_type, NodeType::Bootstrap);
        assert_eq!(result.clandestine_port_opt, Some(1234));
        assert_eq!(result.dns_target, dns_target);
        assert_eq!(result.dns_port, 35);
        assert_eq!(result.earning_wallet, Wallet::new("booga"));
        assert_eq!(
            result.consuming_private_key,
            Some("ABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCDABCD".to_string())
        );
    }

    #[test]
    fn can_make_args() {
        let one_neighbor = NodeReference::new(
            PublicKey::new(&[1, 2, 3, 4]),
            IpAddr::from_str("4.5.6.7").unwrap(),
            vec![1234, 2345],
        );
        let another_neighbor = NodeReference::new(
            PublicKey::new(&[2, 3, 4, 5]),
            IpAddr::from_str("5.6.7.8").unwrap(),
            vec![3456, 4567],
        );

        let subject = NodeStartupConfigBuilder::standard()
            .ip(IpAddr::from_str("1.3.5.7").unwrap())
            .neighbor(one_neighbor.clone())
            .neighbor(another_neighbor.clone())
            .build();

        let result = subject.make_args();

        assert_eq!(
            result,
            Command::strings(vec!(
                "--ip",
                "1.3.5.7",
                "--dns_servers",
                "8.8.8.8",
                "--neighbor",
                format!("{}", one_neighbor).as_str(),
                "--neighbor",
                format!("{}", another_neighbor).as_str(),
                "--wallet_address",
                accountant::DEFAULT_EARNING_WALLET.address.as_str(),
                "--node_type",
                "standard",
                "--log_level",
                "trace",
                "--data_directory",
                "/node_root/home",
            ))
        );
    }
}
