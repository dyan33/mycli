use anyhow::{Ok, Result, bail};
use clap::Parser;
use colored::Colorize;
use md5::compute;
use reqwest::blocking::Client;
use reqwest::header::{REFERER, USER_AGENT};
use serde_json::Value;
use std::fs;
use std::io::BufRead;

/// 英文翻译工具
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// 需要翻译的单词或者句子
    #[arg(index = 1)]
    word: String,

    /// 生词本路径
    #[arg(short, long,default_value="")]
    word_path: String,
}

#[derive(Debug)]
enum Translate {
    En2Zh((), String, Vec<String>),
    Zh2En(String, String),
    SUGGEST(String),
    FANYI(String),
    NOTFOUND,
}

#[inline]
fn md5(str: &str) -> String {
    let digest = compute(str);
    format!("{:x}", digest)
}

fn sign_param(word: &str) -> [(String, String); 6] {
    let r = format!("{word}webdict");
    let time = r.chars().count() % 10;
    let o = md5(&r);
    let n = format!("web{word}{time}Mk6hqtUp33DGGtoS63tTJbMUYjRrG1Lu{o}");
    let sign = md5(&n);

    [
        ("q".into(), word.into()),
        ("le".into(), "en".into()),
        ("t".into(), format!("{time}")),
        ("client".into(), "web".into()),
        ("keyfrom".into(), "webdict".into()),
        ("sign".into(), sign),
    ]
}

fn tranlate(word: &str) -> Result<Translate> {
    let params = sign_param(word);

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

    let fainyi = &value["fanyi"];

    let lang = value["meta"]["guessLanguage"].as_str().unwrap_or_default();

    let trans = if !fainyi.is_null() {
        // 句子翻译
        let tran = fainyi["tran"].as_str().unwrap();
        Translate::FANYI(tran.to_string())
    } else if lang == "eng" && !value["ec"].is_null() {
        // 英-中
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

        Translate::En2Zh((), phonetic.join("\t"), explains)
    } else if lang == "zh" && !value["ce"].is_null() {
        // 中-英
        let value = &value["ce"]["word"];

        let explain: Vec<(&str, &str)> = value["trs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let en = t["#text"].as_str().unwrap();
                let zh = match &t["#tran"] {
                    Value::String(s)=>  &s,
                    _=>"",
                };

                (en, zh)
            })
            .collect();

        let tuple = explain.get(0).unwrap();

        Translate::Zh2En(tuple.0.to_string(), tuple.1.to_string())
    } else if !value["typos"].is_null() {
        // 建议
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

        Translate::SUGGEST(format!("🤔 您要找的是不是:\n\n{}", explain.yellow()))
    } else {
        Translate::NOTFOUND //无法翻译
    };

    Ok(trans)
}

fn pertty_print(trans: &Translate) {
    let line = "--------------------------------------------------------------------";

    println!("🎉 {}", "翻译结果:".green());
    println!("{line}");

    match trans {
        Translate::En2Zh(_, phonetic, explains) => {
            println!("🧑‍🏫 {}", phonetic.magenta());
            println!("{}", "------");
            println!("🇨🇳 {}", explains.join("\n🇨🇳 ").red());
        }
        Translate::Zh2En(world, explain) => {
            println!("🇺🇸 {}", world.magenta());
            println!("{}", "------");
            println!("🇨🇳 {}", explain.red());
        }
        Translate::SUGGEST(explain) => {
            println!("{}", explain.magenta());
        }
        Translate::FANYI(tran) => {
            println!("🧑‍🏫 {}", tran.magenta());
        }
        Translate::NOTFOUND => {
            println!("⚠️ {}", "没有找到".yellow());
        }
    }
    println!("\n{line}");
}

fn save_word_to_csv(word: &str, word_file: &str, trans: &Translate) {
    use chrono::Utc;
    use std::fs::OpenOptions;
    use std::io::{BufReader, Write};

    match trans {
        Translate::En2Zh((), a, explains) => {
            // 记录
            let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

            let explains = explains.join(";");

            let line = format!("{word},{a},{explains},{now}");

            // 目录判断
            let dir = std::path::Path::new(word_file).parent().expect("路径错误");

            if !dir.exists() {
                fs::create_dir_all(dir).unwrap();
            }

            // 写入文件
            let mut file = OpenOptions::new()
                .create(true) // 文件不存在,则创建
                .append(true) // 如果文件存在，则追加内容
                .read(true)
                .open(word_file)
                .unwrap();

            //首次写入表头
            if let None = BufReader::new(&file).lines().next() {
                writeln!(file, "单词,发音,解释,时间").unwrap();
            }

            writeln!(file, "{}", line).unwrap();
        }
        _ => (),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let trans = tranlate(&args.word)?;

    pertty_print(&trans);

    if !args.word_path.is_empty() {
        save_word_to_csv(&args.word, &args.word_path, &trans);
    }

    Ok(())
}

#[cfg(test)]
pub mod tests {

    use crate::tranlate;

    #[test]
    fn test_translate() {
        let value = tranlate("办公室").unwrap();

        println!("response:\n{:?}", value)
    }

    #[test]
    fn test_file_path() {
        use std::path::Path;

        let path = Path::new("youdao.csv");

        println!("path: {}", path.exists())
    }
}
