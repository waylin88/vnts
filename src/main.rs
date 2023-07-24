use std::collections::HashSet;
use std::io::Write;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::net::{TcpListener, UdpSocket};
use crate::service::{start_tcp, start_udp};

pub mod error;
pub mod proto;
pub mod protocol;
pub mod service;

/// 默认网关信息
const GATEWAY: Ipv4Addr = Ipv4Addr::new(10, 26, 0, 1);
const NETMASK: Ipv4Addr = Ipv4Addr::new(255, 255, 255, 0);

/// vnt服务端,
/// 默认情况服务日志输出在 './log/'下,可通过编写'./log/log4rs.yaml'文件自定义日志配置
#[derive(Parser, Debug, Clone)]
pub struct StartArgs {
    /// 指定端口
    #[arg(long)]
    port: Option<u16>,
    /// token白名单，例如 --white-token 1234 --white-token 123
    #[arg(long)]
    white_token: Option<Vec<String>>,
    /// 网关，例如 --gateway 10.10.0.1
    #[arg(long)]
    gateway: Option<String>,
    /// 子网掩码，例如 --netmask 255.255.255.0
    #[arg(long)]
    netmask: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigInfo {
    pub port: u16,
    pub white_token: Option<HashSet<String>>,
    pub gateway: Ipv4Addr,
    pub broadcast: Ipv4Addr,
    pub netmask: Ipv4Addr,
}

fn log_init() {
    let log_path = PathBuf::from("log");
    if !log_path.exists() {
        let _ = std::fs::create_dir(&log_path);
    }
    let log_config = log_path.join("log4rs.yaml");
    if !log_config.exists() {
        if let Ok(mut f) = std::fs::File::create(&log_config) {
            let _ = f.write_all(b"refresh_rate: 30 seconds
appenders:
  rolling_file:
    kind: rolling_file
    path: log/vnts.log
    append: true
    encoder:
      pattern: \"{d(%+)(utc)} [{f}:{L}] {h({l})} {M}:{m}{n}\"
    policy:
      kind: compound
      trigger:
        kind: size
        limit: 10 mb
      roller:
        kind: fixed_window
        pattern: log/vnts.{}.log
        base: 1
        count: 5

root:
  level: info
  appenders:
    - rolling_file");
        }
    }
    let _ = log4rs::init_file(log_config, Default::default());
}

#[tokio::main]
async fn main() {
    let args = StartArgs::parse();
    let port = args.port.unwrap_or(29871);
    println!("端口：{}", port);
    let white_token = if let Some(white_token) = args.white_token {
        Some(HashSet::from_iter(white_token.into_iter()))
    } else {
        None
    };
    println!("token白名单：{:?}", white_token);
    let gateway = if let Some(gateway) = args.gateway {
        gateway.parse::<Ipv4Addr>().expect("网关错误，必须为有效的ipv4地址")
    } else {
        GATEWAY
    };
    println!("网关：{:?}", gateway);
    if gateway.is_unspecified() {
        println!("网关地址无效");
        return;
    }
    if gateway.is_broadcast() {
        println!("网关错误，不能为广播地址");
        return;
    }
    if gateway.is_multicast() {
        println!("网关错误，不能为组播地址");
        return;
    }
    if !gateway.is_private() {
        println!("Warning 不是一个私有地址：{:?}，将有可能和公网ip冲突", gateway);
    }
    let netmask = if let Some(netmask) = args.netmask {
        netmask.parse::<Ipv4Addr>().expect("子网掩码错误，必须为有效的ipv4地址")
    } else {
        NETMASK
    };
    println!("子网掩码：{:?}", netmask);
    if netmask.is_broadcast() || netmask.is_unspecified() || !(!u32::from_be_bytes(netmask.octets()) + 1).is_power_of_two() {
        println!("子网掩码错误");
        return;
    }

    let broadcast = (!u32::from_be_bytes(netmask.octets()))
        | u32::from_be_bytes(gateway.octets());
    let broadcast = Ipv4Addr::from(broadcast);
    let config = ConfigInfo {
        port,
        white_token,
        gateway,
        broadcast,
        netmask,
    };
    log_init();
    log::info!("config:{:?}",config);
    let udp = match UdpSocket::bind(format!("0.0.0.0:{}", port)).await {
        Ok(udp) => { Arc::new(udp) }
        Err(e) => {
            log::warn!("udp启动失败:{:?}",e);
            panic!("{:?}", e);
        }
    };
    log::info!("监听udp端口:{:?}",udp.local_addr().unwrap());
    println!("监听udp端口:{:?}", udp.local_addr().unwrap());
    let tcp = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(tcp) => { tcp }
        Err(e) => {
            log::warn!("tcp启动失败:{:?}",e);
            panic!("{:?}", e);
        }
    };
    log::info!("监听tcp端口:{:?}",tcp.local_addr().unwrap());
    println!("监听tcp端口:{:?}", tcp.local_addr().unwrap());
    let config = config.clone();
    let main_udp = udp.clone();
    let tcp_config = config.clone();
    tokio::spawn(async move {
        if let Err(e) = start_tcp(tcp, main_udp, tcp_config).await {
            log::warn!("tcp任务结束:{:?}",e);
        }
    });
    start_udp(udp, config).await;
}
