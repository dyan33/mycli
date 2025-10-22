use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use reqwest::blocking::Client;
use reqwest::header::USER_AGENT;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use url::Url;
use urlencoding::decode;
use clap::Parser;
use std::path::PathBuf;
use tracing::{info, Level};     // 日志记录
use tracing_subscriber;        // 日志订阅者


fn parse_ss(url: Url) -> Value {
    let tag = url.fragment().unwrap();
    let tag = decode(tag).unwrap();

    json!({
        "type": "shadowsocks",
        "tag": tag,
        "server": url.host_str().unwrap(),
        "server_port": url.port().unwrap(),
        "password": url.password().unwrap(),
        "method": url.username(),
    })
}
fn parse_trojan(url: Url) -> Value {
    let tag = url.fragment().unwrap();
    let tag = decode(tag).unwrap();
    let query = url.query_pairs();
    let query: HashMap<_, _> = query.into_iter().collect();

    json!({
        "type": "trojan",
        "server": url.host_str().unwrap(),
        "server_port": url.port().unwrap(),
        "tag": tag,
        "password": url.username(),
        "network": query.get("type").unwrap(),
        "tls": {
            "enabled": true,
            "server_name": query.get("sni"),
            "insecure": true,
            "utls": {
                "enabled": true,
                "fingerprint": "chrome"
            },
        },
    })
}
fn parse_vless(url: Url) -> Value {
    let tag = url.fragment().unwrap();
    let tag = decode(tag).unwrap();

    let query: HashMap<_, _> = url.query_pairs().into_iter().collect();

    let fp = query.get("fp").map(|v| v.to_string()).unwrap();

    let network=query.get("type").unwrap().to_string();

    let network= if network=="ws" {"tcp"} else {&network};

    json!({
        "type": "vless",
        "tag": tag,
        "server": url.host_str().unwrap(),
        "server_port": url.port(),
        "uuid": url.username(),
        "flow": query.get("flow"),
        "network": network,
        "tls": {
            "enabled": true,
            "server_name": query.get("sni"),
            "reality": {
                "enabled": query.get("security").map(|v| v.to_string()).unwrap()=="reality",
                "public_key": query.get("pbk"),
                "short_id": query.get("sid"),
            },
            "utls": {"enabled": !fp.is_empty(), "fingerprint": fp},
        },
    })
}
fn parse_hysteria2(url: Url) -> Value {
    let tag = url.fragment().unwrap();
    let tag = decode(tag).unwrap();

    let query: HashMap<_, _> = url.query_pairs().into_iter().collect();

    let password = if url.password() == None {
        url.username()
    } else {
        &format!("{}:{}", url.username(), url.password().unwrap())
    };

    json!( {
        "type": "hysteria2",
        "tag": tag,
        "server": url.host_str().unwrap(),
        "server_port": url.port().unwrap(),
        // "server_ports": [p.replace("-", ":") for p in q.get("mport", [""])],
        "password": password ,
        "tls": {
            "enabled": true,
            "server_name": query.get("sni"),
            "insecure": query.get("insecure").unwrap_or(&Cow::Owned(String::new())) == "1",
        },
    })
}
fn load_tmeplate(path: &str) -> Value {
    let content = fs::read_to_string(path).expect("读取配置文件失败");
    let value: Value = serde_json::from_str(&content).expect("解析配置文件失败");
    value
}

fn parse_urls(urls: Vec<&str>) -> Vec<Value> {
    let nodes: Vec<Vec<Value>> = urls
        .iter()
        .map(|url| {
            let client = Client::new();

            let response = client.get(*url)
            .header(USER_AGENT, "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36")
            .send()
            .expect(&format!("请求链接失败: {:?}",url));

            let sub_url=Url::parse(&url).unwrap();

            let sub_name= if let Some(name)=sub_url.fragment() {
                Some(decode(name).unwrap().into_owned())
            } else {
                None
            };


            assert!(response.status().is_success());

            let content = response.text().unwrap();

            let bytes = BASE64_STANDARD.decode(content).unwrap();

            let content = String::from_utf8(bytes).unwrap();

            let values: Vec<Value> = content
                .split("\r\n")
                .filter(|s| !s.is_empty())
                .map(|line| {

                    let url_parsed = Url::parse(line).unwrap();

                    let value = match url_parsed.scheme() {
                        "ss" => parse_ss(url_parsed),
                        "trojan" => parse_trojan(url_parsed),
                        "vless" => parse_vless(url_parsed),
                        "hysteria2" => parse_hysteria2(url_parsed),
                        _ => Value::Null,
                    };

                    if let Some(name)=&sub_name {

                        let mut val=value;

                        let tag=val["tag"].as_str().unwrap_or_default();

                        let tag=format!("{}:{}",name,tag);

                        val["tag"]=tag.into();

                        return val;
                    }
                    value
                })
                .collect();
            return values;
        })
        .collect();

    let nodes: Vec<Value> = nodes.into_iter().flat_map(|v| v).collect();
    nodes
}

fn singbox_config(base_conf_path: &str, urls: Vec<&str>, ignores: Vec<&str>) -> Value {
    let mut config = load_tmeplate(base_conf_path);

    // 有效的节点
    let nodes: Vec<Value> = parse_urls(urls)
        .iter()
        .filter(|v| {

            let tag = v["tag"].as_str().unwrap();

            if v["server"].as_str().unwrap_or_default().eq("0.0.0.0") || ignores.iter().any(|kw| tag.contains(kw)){

                info!("过滤节点: {}",tag);

                return false;
            }

            true
        })
        .map(|v| v.clone())
        .collect();

    let node_tags: Vec<_> = nodes.iter().map(|v| v["tag"].clone()).collect();

    // AI节点过滤
    let ai_node_tags: Vec<_> = node_tags
        .iter()
        .filter(|v| {
            let tag = v.as_str().unwrap();

            let names = [
                "美国",
                "日本",
                "新加坡",
                "韩国",
                "英国",
                "德国",
                "法国",
                "加拿大",
            ];
            return names.iter().any(|n| tag.contains(n));
        })
        .map(|v| v.clone())
        .collect();

    //组合配置
    for out in config["outbounds"].as_array_mut().unwrap() {
        let tag = out["tag"].as_str().unwrap();

        if tag == "自动选择" {
            let outbounds = out["outbounds"].as_array_mut().unwrap();
            outbounds.extend_from_slice(&node_tags);
        } else if tag == "OPENAI" {
            let outbounds = out["outbounds"].as_array_mut().unwrap();
            outbounds.extend_from_slice(&ai_node_tags);
        }
    }

    config["outbounds"]
        .as_array_mut()
        .unwrap()
        .extend_from_slice(&nodes);

    config
}


/// singbox订阅转换工具
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct  Args{

    /// 订阅链接
    #[arg(short, long,required=true)]
    urls: Vec<String>,

    /// 基本配置
    #[arg(short, long,required=true)]
    base_confg:String,

    /// 保存位置
    #[arg(short, long,default_value="~/.config/mycli/singbox/config.json")]
    save_config:String,

    /// 过滤关键词
    #[arg(short, long,default_values=[
        "剩余流量",
        "套餐到期",
        ])]
    ingores:Vec<String>,


    #[arg(short,long,default_value="false")]
    verbose:bool


}

fn main() {

    let args = Args::parse();

    if args.verbose {
        // 详细日志模式
        tracing_subscriber::fmt()
                    .with_max_level(Level::DEBUG)
                    .init();
    }else {
       tracing_subscriber::fmt()
                    .with_max_level(Level::WARN)
                    .init();
    }


    let config: Value=singbox_config(
        &args.base_confg,
         args.urls.iter().map(|s|s.as_str()).collect(),
        args.ingores.iter().map(|s|s.as_str()).collect(),
    );


    let path= if args.save_config.starts_with("~/") {
        dirs::home_dir().expect("获取Home目录失败").join(&args.save_config[2..])
     }else {
         PathBuf::from(args.save_config)
     };

    if let Some(parent)=path.parent() && !parent.exists() {
        fs::create_dir_all(parent).expect(&format!("创建目录失败: {:?}",parent))
    }

    let path=path.as_path();

    fs::write(path, serde_json::to_string_pretty(&config).unwrap()).expect(&format!("保存文件失败: {:?}",path));

    info!("保存至: {:?}",path);
}
