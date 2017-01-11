use std::str;
use std::net::UdpSocket;

extern crate hyper;
extern crate telegram_bot;
extern crate json;

struct Emitter {
    api: telegram_bot::Api
}

impl Emitter {
    fn new() -> Emitter {
        let api = telegram_bot::Api::from_env("TELEGRAM_TOKEN").unwrap();

        Emitter {
            api: api
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
            page, evt_curid, evt_previd
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
            page, evt_curid
        );

        let msg = format!(
            "[created page] [{}]{}{}{} [{}] {} - {}",
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

    fn handle_evt_log(&self, evt: &json::JsonValue) {
        let log_type = evt["log_type"].to_string();

        if log_type == "thanks" {
            return self.handle_evt_log_thanks(evt);
        }

        if log_type == "patrol" {
            return self.handle_evt_log_patrol(evt);
        }

        if log_type == "profile" {
            return self.handle_evt_log_profile(evt);
        }

        // not implemented
        if log_type != "null" {
            let msg = format!(
                "[log] [not_implemented] {}",
                evt.dump()
            );

            self.emit(msg);
        }
    }

    fn handle_evt_log_thanks(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            "[log] [thanks] {:?}",
            comment
        );

        self.emit(msg);
    }

    fn handle_evt_log_profile(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();
        let user = evt["user"].to_string();

        let msg = format!(
            "[log] [profile] [{}] {}",
            user, comment
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
            page, evt_curid, evt_previd
        );

        let msg = format!(
            "[log] [patrol] [{}] {}- {}",
            user, comment, url
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
