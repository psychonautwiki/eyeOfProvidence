use std::str;
use std::net::UdpSocket;

extern crate hyper;
extern crate telegram_bot;
extern crate json;
extern crate regex;

extern crate url;

use regex::Regex;

use url::percent_encoding::{
    percent_encode, QUERY_ENCODE_SET
};

struct EmitterRgx {
    percent_rgx: regex::Regex
}

impl EmitterRgx {
    fn new() -> EmitterRgx {
        let percent_rgx = Regex::new(r"%").unwrap();

        EmitterRgx {
            percent_rgx: percent_rgx
        }
    }

    fn percent_to_url(&self, orig: &str) -> String {
        self.percent_rgx.replace_all(orig, "%25").to_string()
    }
}

struct Emitter {
    api: telegram_bot::Api,
    emitter_rgx: EmitterRgx
}

impl Emitter {
    fn new() -> Emitter {
        let api = telegram_bot::Api::from_env("TELEGRAM_TOKEN").unwrap();

        let emitter_rgx = EmitterRgx::new();

        Emitter {
            api: api,
            emitter_rgx: emitter_rgx
        }
    }

    fn emit(&self, msg: String) {
        let _ = self.api.send_message(
            -1001050593583 as i64,
            msg,
            None, None, None, None
        );
    }

    fn handle_evt(&self, evt: &json::JsonValue) {
        let evt_type = evt["type"].to_string();

        if evt_type == "edit" {
            return self.handle_evt_edit(evt);
        }

        if evt_type == "new" {
            return self.handle_evt_new(evt);
        }

        if evt_type == "log" {
            return self.handle_evt_log(evt);
        }

        // not implemented
        if evt_type != "null" {
            let msg = format!(
                "[not_implemented] {}",
                evt.dump()
            );

            self.emit(msg);
        }
    }

    fn urlencode(orig: &str) -> String {
        percent_encode(orig.as_bytes(), QUERY_ENCODE_SET).collect::<String>()
    }

    // do an additional encode on top of urlencode
    // as the url crate doesn't allow for double-encode
    // as per ISO specification
    fn wrap_urlencode(&self, orig: &str) -> String {
        self.emitter_rgx.percent_to_url(orig)
    }

    fn cond_string(cond: bool, protagonist: &str, antagonist: &str) -> String {
        match cond {
            true => protagonist.to_string(),
            false => antagonist.to_string()
        }
    }

    fn explain_comment(comment: &str) -> String {
        if comment == "" {
            return format!("without summary");
        }

        return format!("with summary: {:?}", comment);
    }

    fn handle_evt_edit(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();
        let comment = evt["comment"].to_string();

        let evt_curid = evt["revision"]["new"].as_u32().unwrap();
        let evt_previd = evt["revision"]["old"].as_u32().unwrap();

        let evt_is_minor = evt["minor"].as_bool().unwrap();
        let evt_is_patrolled = evt["patrolled"].as_bool().unwrap();
        let evt_is_bot = evt["bot"].as_bool().unwrap();

        let url = format!(
            "https://psychonautwiki.org/w/index.php?title={}%26type=revision%26diff={:?}%26oldid={:?}",
            self.wrap_urlencode(&Emitter::urlencode(&page)), evt_curid, evt_previd
        );

        let msg = format!(
            "[edit] [{}]{}{}{} [{}] {} - {}",
            user,

            Emitter::cond_string(evt_is_minor, " [minor]", ""),
            Emitter::cond_string(evt_is_patrolled, " [auto_patrolled]", ""),
            Emitter::cond_string(evt_is_bot, " [bot]", ""),

            page,

            Emitter::explain_comment(&comment),

            url
        );

        self.emit(msg);
    }

    fn handle_evt_new(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();
        let comment = evt["comment"].to_string();

        let evt_curid = evt["revision"]["new"].as_u32().unwrap();

        let evt_is_minor = evt["minor"].as_bool().unwrap();
        let evt_is_patrolled = evt["patrolled"].as_bool().unwrap();
        let evt_is_bot = evt["bot"].as_bool().unwrap();

        let url = format!(
            "https://psychonautwiki.org/w/index.php?title={}%26oldid={:?}",
            self.wrap_urlencode(&Emitter::urlencode(&page)), evt_curid
        );

        let msg = format!(
            "[created page] [{}]{}{}{} [{}] {} - {}",
            user,

            Emitter::cond_string(evt_is_minor, " [minor]", ""),
            Emitter::cond_string(evt_is_patrolled, " [auto_patrolled]", ""),
            Emitter::cond_string(evt_is_bot, " [bot]", ""),

            page, comment, url
        );

        self.emit(msg);
    }

    fn handle_evt_log(&self, evt: &json::JsonValue) {
        let log_type = evt["log_type"].to_string();

        if log_type == "avatar" {
            return self.handle_evt_log_avatar(evt);
        }

        if log_type == "block" {
            return self.handle_evt_log_block(evt);
        }

        if log_type == "delete" {
            return self.handle_evt_log_delete(evt);
        }

        if log_type == "move" {
            return self.handle_evt_log_move(evt);
        }

        if log_type == "newusers" {
            return self.handle_evt_log_newusers(evt);
        }

        if log_type == "patrol" {
            return self.handle_evt_log_patrol(evt);
        }

        if log_type == "profile" {
            return self.handle_evt_log_profile(evt);
        }

        if log_type == "thanks" {
            return self.handle_evt_log_thanks(evt);
        }

        if log_type == "upload" {
            return self.handle_evt_log_upload(evt);
        }

        // not implemented
        if log_type != "null" {
            let msg = format!(
                "[log_not_implemented] {}",
                evt.dump()
            );

            self.emit(msg);
        }
    }

    fn handle_evt_log_avatar(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();
        let comment = evt["comment"].to_string();

        let url_page = self.wrap_urlencode(&Emitter::urlencode(&page));

        let msg = format!(
            "[log/avatar] [{}] {} - https://psychonautwiki.org/wiki/{}",

            user, comment, url_page
        );

        self.emit(msg);
    }

    fn handle_evt_log_block(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            "[log/ban] [{}] {}",

            user, comment
        );

        self.emit(msg);
    }

    fn handle_evt_log_delete(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();
        let comment = evt["comment"].to_string();

        let url_page = self.wrap_urlencode(&Emitter::urlencode(&page));

        let msg = format!(
            "[log/delete] [{}] deleted page: {:?} with comment: {:?} - https://psychonautwiki.org/wiki/{}",

            user, page, comment, url_page
        );

        self.emit(msg);
    }

    fn handle_evt_log_move(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();

        let evt_target = evt["log_params"]["target"].to_string();

        let url_page = self.wrap_urlencode(&Emitter::urlencode(&evt_target));

        let msg = format!(
            "[log/move] [{}] moved {:?} to {:?} - https://psychonautwiki.org/wiki/{}",

            user, page, evt_target, url_page
        );

        self.emit(msg);
    }

    fn handle_evt_log_patrol(&self, evt: &json::JsonValue) {
        if !evt["log_params"]["auto"].is_number() {
            return;
        }

        let evt_auto = evt["log_params"]["auto"].as_u32().unwrap();

        if evt_auto == 1u32 {
            return;
        }

        let evt_curid = evt["log_params"]["curid"].as_u32().unwrap();
        let evt_previd = evt["log_params"]["previd"].as_u32().unwrap();

        let user = evt["user"].to_string();
        let page = evt["title"].to_string();
        let comment = evt["log_action_comment"].to_string();

        let url = format!(
            "https://psychonautwiki.org/w/index.php?title={}%26type=revision%26diff={:?}%26oldid={:?}",
            self.wrap_urlencode(&Emitter::urlencode(&page)), evt_curid, evt_previd
        );

        let msg = format!(
            "[log/patrol] [{}] {}- {}",
            user, comment, url
        );

        self.emit(msg);
    }

    fn handle_evt_log_profile(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();
        let user = evt["user"].to_string();

        let msg = format!(
            "[log/profile] [{}] {}",
            user, comment
        );

        self.emit(msg);
    }

    fn handle_evt_log_newusers(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();

        let user = evt["user"].to_string();
        let user_page = evt["title"].to_string();

        let msg = format!(
            "[log/newusers] [{}] {} - https://psychonautwiki.org/wiki/{}",
            user, comment, self.wrap_urlencode(&Emitter::urlencode(&user_page))
        );

        self.emit(msg);
    }

    fn handle_evt_log_upload(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();

        let user = evt["user"].to_string();
        let user_page = evt["title"].to_string();

        let url_page = self.wrap_urlencode(&Emitter::urlencode(&user_page));

        let msg = format!(
            "[log/upload] [{}] uploaded file: {:?} - https://psychonautwiki.org/wiki/{}",
            user, user_page, url_page
        );

        self.emit(msg);
    }

    fn handle_evt_log_thanks(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            "[log/thanks] {:?}",
            comment
        );

        self.emit(msg);
    }
}

fn main() {
    println!("~~~~~~ 6EQUJ5 ~~~~~~");

    let emitter = Emitter::new();

    let socket = match UdpSocket::bind("0.0.0.0:3000") {
        Ok(s) => s,
        Err(e) => panic!("couldn't bind socket: {}", e)
    };

    let mut buf = [0; 2048];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((amt, _)) => {
                let instr = str::from_utf8(&buf[0..amt]).unwrap_or("");

                let evt = json::parse(instr);

                if !evt.is_ok() {
                    continue;
                }

                let ref evt = evt.unwrap();

                emitter.handle_evt(evt);
            },
            Err(e) => {
                println!("couldn't recieve a datagram: {}", e);
            }
        }
    }
}
