use anyhow::{Ok, Result, bail};
use clap::Parser;
use colored::Colorize;
use reqwest::blocking::Client;
use reqwest::header::{REFERER, USER_AGENT};
use serde_json::Value;
use tracing::{Level};

/// 有英文道翻译工具
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// 需要翻译的单词
    #[arg(short, long, required = true)]
    word: String,

    /// 配置文件
    #[arg(short, long, default_value = "~/.config/youdao/youdao.csv")]
    dict_path: String,

    ///  打印详细日志
    #[arg(short, long, default_value = "false")]
    verbose: bool,
}

#[derive(Debug)]
enum Translate {
    EnZh((), String, Vec<String>),
    ZhEn(String, String),
    OTHER(String),
}

fn tranlate(world: &str) -> Result<Translate> {
    let params = [
        ("q", world),
        ("le", "en"),
        ("t", "3"),
        ("client", "web"),
        ("keyform", "webdict"),
    ];

    let response=Client::new()
    .post("https://dict.youdao.com/jsonapi_s?doctype=json&jsonversion=4")
    .header(USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/93.0.4577.63 Safari/537.36")
    .header(REFERER,"https://youdao.com/")
    .form(&params)
    .send()?;

    let success = response.status().is_success();

    let value: Value = response.json()?;

    if !success {
        bail!(value)
    }

    let lang = value["meta"]["guessLanguage"].as_str().unwrap_or_default();

    let trans = if lang == "eng" && !value["ec"].is_null() {
        let value = &value["ec"]["word"];

        let mut phonetic: Vec<String> = Vec::with_capacity(2);

        if !value["usphone"].is_null() {
            let str = format!("美/{}/", value["usphone"].as_str().unwrap());

            phonetic.push(str);
        }
        if !value["ukphone"].is_null() {
            let str = format!("英/{}/", value["ukphone"].as_str().unwrap());

            phonetic.push(str);
        }
        let explains: Vec<String> = value["trs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let tran = t["tran"].as_str().unwrap();

                let pos = if !t["pos"].is_null() {
                    t["pos"].as_str().unwrap()
                } else {
                    "."
                };

                format!("{} {}", pos, tran)
            })
            .collect();

        Translate::EnZh((), phonetic.join("\t"),explains)
    } else if lang == "zh" && !value["ce"].is_null() {
        let value = &value["ce"]["word"];

        let explain: Vec<(&str,&str)> = value["trs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let en = t["#text"].as_str().unwrap();
                let zh = t["#tran"].as_str().unwrap();

                (en,zh)
            })
            .collect();

        let tuple=explain.get(0).unwrap();

        Translate::ZhEn(tuple.0.to_string(),tuple.1.to_string())
    } else {
        let typo = &value["typos"]["typo"];

        let explain: Vec<String> = typo
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let zh = t["trans"].as_str().unwrap();
                let en = t["word"].as_str().unwrap();

                format!("🇬🇧 {}\n🇨🇳 {}", en, zh)
            })
            .collect();

        let explain = explain.join("\n------\n");

        Translate::OTHER(format!("🤔 您要找的是不是:\n\n{}",explain.yellow()))
    };

    Ok(trans)
}

fn println(trans: &Translate) {
    let line = "--------------------------------------------------------------------";

    println!("🎉 {}", "翻译结果:".green());
    println!("{line}");

    match trans {
        Translate::EnZh(_, phonetic, explains) => {
            println!("🧑‍🏫 {}", phonetic.magenta());
            println!("{}", "------");
            println!("🇨🇳 {}", explains.join("\n🇨🇳 ").red());
        }
        Translate::ZhEn(world, explain) => {
            println!("🇺🇸 {}", world.magenta());
            println!("{}", "------");
            println!("🇨🇳 {}", explain.red());
        }
        Translate::OTHER(explain) => {
            println!("{}", explain.red());
        }
    }
    println!("\n{line}");
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.verbose {
        // 详细日志模式
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt().with_max_level(Level::WARN).init();
    }

    let trans = tranlate(&args.word)?;

    println(&trans);

    Ok(())
}

#[cfg(test)]
pub mod tests {

    use crate::tranlate;

    #[test]
    fn test_translate() {
        let value = tranlate("pear").unwrap();

        println!("response:\n{:?}", value)
    }
}