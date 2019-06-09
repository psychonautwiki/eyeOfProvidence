extern crate futures;

extern crate tokio_core;

extern crate telegram_bot;
use telegram_bot::prelude::*;

extern crate json;

extern crate afterparty_ng as afterparty;
use afterparty::{Delivery, Hub};

extern crate hyper;
use hyper::{Client, Server};

use hyper::net::HttpsConnector;
use hyper_native_tls::NativeTlsClient;

extern crate url;
use url::percent_encoding::{
    percent_encode, percent_decode, QUERY_ENCODE_SET
};

extern crate htmlescape;

extern crate regex;
use regex::Regex;

use std::{
    io::Read,
    net::UdpSocket
};

extern crate scoped_threadpool;
use scoped_threadpool::Pool;

#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate serde_qs;

#[macro_use]
extern crate rouille;

const MEDIAWIKI_ENDPOINT: &'static str = "0.0.0.0:3000";
const GITHUB_ENDPOINT: &'static str = "0.0.0.0:4567";
const JIRA_ENDPOINT: &'static str = "0.0.0.0:9293";
const PAYPAL_ENDPOINT: &'static str = "0.0.0.0:9728";

const PW_API_URL_PREFIX: &'static str = "https://psychonautwiki.org/w/api.php";

// SANDBOX
//const PAYPAL_IPN_VERIFY_URL: &'static str = "https://ipnpb.sandbox.paypal.com/cgi-bin/webscr?cmd=_notify-validate&";
// LIVE
const PAYPAL_IPN_VERIFY_URL: &'static str = "https://ipnpb.paypal.com/cgi-bin/webscr?cmd=_notify-validate&";

fn verify_paypal_ipn (ipn_payload: impl Into<String>) -> bool {
    let ssl = NativeTlsClient::new().unwrap();
    let connector = HttpsConnector::new(ssl);

    let client = Client::with_connector(connector);

    let url = format!("{}{}", PAYPAL_IPN_VERIFY_URL, ipn_payload.into());

    let res = client.get(
        &url
    ).send();

    if !res.is_ok() {
        let _ = res.map_err(|err| println!("{:?}", err));
        return false;
    }

    let mut res = res.unwrap();

    let mut buf = Vec::new();

    match res.read_to_end(&mut buf) {
        Ok(_) => {},
        _ => {
            return false;
        }
    }

    &buf == b"VERIFIED"
}

fn legacy_hyper_load_url (url: String) -> Option<json::JsonValue> {
    let client = Client::new();

    let res = client.get(&url).send();

    if !res.is_ok() {
        return None;
    }

    let mut res = res.unwrap();

    let mut buf = String::new();

    match res.read_to_string(&mut buf) {
        Ok(_) => {},
        _ => {
            return None;
        }
    }

    match json::parse(&buf) {
        Ok(data) => Some(data),
        Err(_) => None
    }
}

#[derive(Debug)]
struct RevInfo (String, String, String);

fn get_revision_info(title: String, rev_id: String) -> Option<RevInfo> {
    let title = title;
    let rev_id = rev_id;

    let url = format!(
        "{}?action=query&prop=revisions&titles={}&rvprop=timestamp%7Cuser%7Ccomment%7Ccontent%7Cids&rvstartid={}&rvendid={}&format=json",

        PW_API_URL_PREFIX,
        title,
        rev_id,
        rev_id
    );

    let revision_data = legacy_hyper_load_url(url);

    if !revision_data.is_some() {
        return None;
    }

    let revision_data = revision_data.unwrap();

    let pages = &revision_data["query"]["pages"];

    // "-1" is used to denote "not found" in mediawiki
    if pages.has_key("-1") || pages.len() != 1usize {
        return None;
    }

    // Obtain page_id

    let mut entry: &str = "";

    for (key, _) in pages.entries() {
        entry = key;
    }

    // try to obtain the target revision
    let results = &pages[entry]["revisions"][0].clone();

    if results.is_empty() {
        return None;
    }

    Some(
        RevInfo(
            results["user"].to_string(),
            results["comment"].to_string(),
            results["parentid"].to_string()
        )
    )
}

struct EmitterRgx {
    percent_rgx: regex::Regex,
    plus_rgx: regex::Regex,
    and_rgx: regex::Regex,
    questionmark_rgx: regex::Regex,
}

impl EmitterRgx {
    fn new() -> EmitterRgx {
        let percent_rgx = Regex::new(r"%").unwrap();
        let plus_rgx = Regex::new(r"\+").unwrap();
        let and_rgx = Regex::new(r"\&").unwrap();
        let questionmark_rgx = Regex::new(r"\?").unwrap();

        EmitterRgx {
            percent_rgx,
            plus_rgx,
            and_rgx,
            questionmark_rgx,
        }
    }

    fn percent_to_url(&self, orig: &str) -> String {
        self.percent_rgx.replace_all(orig, "%25").to_string()
    }

    fn plusexclquest_to_url(&self, orig: &str) -> String {
        let orig_pr = self.plus_rgx.replace_all(orig, "%2b").to_string();
        let orig_ar = self.and_rgx.replace_all(&orig_pr, "%26").to_string();

        self.questionmark_rgx.replace_all(&orig_ar, "%3f").to_string()
    }
}

struct ConfiguredApi {
    api: telegram_bot::Api,
    core: std::cell::RefCell<tokio_core::reactor::Core>,
    channel_id: i64,
    name: String,
    parse_mode: telegram_bot::types::ParseMode
}

fn htmlescape_str<T: Into<String>>(msg: T) -> String {
    let msg = msg.into();
    let mut writer = Vec::with_capacity((msg.len()/3 + 1) * 4);

    match htmlescape::encode_minimal_w(&msg, &mut writer) {
        Err(_) => {
            println!("Could not html-encode string: {:?}", msg);

            msg
        },
        Ok(_) =>
            match String::from_utf8(writer) {
                Ok(encoded_msg) => encoded_msg,
                _ => msg
            }
    }
}

impl ConfiguredApi {
    fn new(name: &str, parse_mode: telegram_bot::types::ParseMode) -> ConfiguredApi {
        let core = tokio_core::reactor::Core::new().unwrap();

        let token = std::env::var("TELEGRAM_TOKEN").unwrap();
        let api = telegram_bot::Api::configure(token).build(core.handle()).unwrap();

        ConfiguredApi {
            api,
            core: std::cell::RefCell::new(core),

            channel_id: -1001050593583,
            name: name.to_string(),
            parse_mode
        }
    }

    fn emit<T: Into<String>>(&self, msg: T, should_notify: bool) {
        let msg = format!("⥂ {} ⟹ {}", self.name, msg.into());

        let channel = telegram_bot::ChannelId::new(self.channel_id);

        let mut chan_msg = channel.text(msg);

        let msg_op = chan_msg
            .parse_mode(self.parse_mode)
            .disable_preview();

        let msg_op_notif = match should_notify {
            true => msg_op,
            false => msg_op.disable_notification()
        };

        let tg_future  = self.api.send(
            msg_op_notif
        );

        let _ = self.core.borrow_mut().run(tg_future);
    }
}

/*
 * MEDIAWIKI UDP CHANGE EVENTS
 */

struct MediaWikiEmitter {
    configured_api: ConfiguredApi,
    emitter_rgx: EmitterRgx
}

impl MediaWikiEmitter {
    fn new() -> MediaWikiEmitter {
        let configured_api = ConfiguredApi::new(&"<b>MediaWiki</b>", telegram_bot::types::ParseMode::Html);

        let emitter_rgx = EmitterRgx::new();

        MediaWikiEmitter {
            configured_api,
            emitter_rgx
        }
    }

    fn handle_evt(&self, evt: &json::JsonValue) {
        let evt_type = evt["type"].to_string();

        match &*evt_type {
            "edit" => self.handle_evt_edit(evt),
            "log" => self.handle_evt_log(evt),
            "new" => self.handle_evt_new(evt),
            _ => {
                if evt_type == "null" {
                    return;
                }

                let msg = format!(
                    "[not_implemented] {}",
                    evt.dump()
                );

                self.configured_api.emit(msg, true);
            }
        }
    }

    fn urlencode(orig: &str) -> String {
        percent_encode(orig.as_bytes(), QUERY_ENCODE_SET).collect::<String>()
    }

    fn urldecode(orig: &str) -> String {
        String::from_utf8(
            percent_decode(orig.as_bytes()).collect::<Vec<u8>>()
        ).unwrap_or("<string conversion failed>".to_string())
    }

    // do an additional encode on top of urlencode
    // as the url crate doesn't allow for double-encode
    // as per ISO specification
    fn wrap_urlencode(&self, orig: &str) -> String {
        self.emitter_rgx.percent_to_url(orig)
    }

    fn get_user_url(&self, user: &str) -> String {
        let target = format!(
            "User:{}",

            user
        );

        self.get_url(&target)
    }

    fn get_url(&self, page: &str) -> String {
        let url = format!(
            "https://psychonautwiki.org/wiki/{}",

            page
        );

        url
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

        return format!("with summary: {}", comment);
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
            "https://psychonautwiki.org/w/index.php?title={}&type=revision&diff={:?}&oldid={:?}",
            self.wrap_urlencode(&MediaWikiEmitter::urlencode(&page)), evt_curid, evt_previd
        );

        let has_flags = evt_is_minor || evt_is_patrolled || evt_is_bot;

        let flags = format!(
            "{}{}{}",

            MediaWikiEmitter::cond_string(evt_is_minor, "<b>minor</b> ", ""),
            MediaWikiEmitter::cond_string(evt_is_patrolled, "<b>patrolled</b> ", ""),
            MediaWikiEmitter::cond_string(evt_is_bot, "<b>bot</b> ", "")
        );

        let msg = format!(
            r#"{}<a href="{}">{}</a> edited <a href="{}">{}</a> {}"#,

            MediaWikiEmitter::cond_string(
                has_flags,
                &format!("| {}| ", flags),
                ""
            ),

            self.get_user_url(&user),
            user,

            url,
            page,

            MediaWikiEmitter::explain_comment(&comment)
        );

        self.configured_api.emit(msg, true);
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
            "https://psychonautwiki.org/w/index.php?title={}&oldid={:?}",
            self.wrap_urlencode(&MediaWikiEmitter::urlencode(&page)), evt_curid
        );

        let has_flags = evt_is_minor || evt_is_patrolled || evt_is_bot;

        let flags = format!(
            "{}{}{}",

            MediaWikiEmitter::cond_string(evt_is_minor, "<b>minor</b> ", ""),
            MediaWikiEmitter::cond_string(evt_is_patrolled, "<b>patrolled</b> ", ""),
            MediaWikiEmitter::cond_string(evt_is_bot, "<b>bot</b> ", "")
        );

        let msg = format!(
            r#"[new] {}<a href="{}">{}</a> created page <a href="{}">{}</a> {}"#,

            MediaWikiEmitter::cond_string(
                has_flags,
                &format!("| {}| ", flags),
                ""
            ),

            self.get_user_url(&user),
            user,

            url,
            page,

            MediaWikiEmitter::explain_comment(&comment)
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log(&self, evt: &json::JsonValue) {
        let log_type = evt["log_type"].to_string();

        match &*log_type {
            "approval" => self.handle_evt_log_approval(evt),
            "avatar" => self.handle_evt_log_avatar(evt),
            "block" => self.handle_evt_log_block(evt),
            "delete" => self.handle_evt_log_delete(evt),
            "move" => self.handle_evt_log_move(evt),
            "newusers" => self.handle_evt_log_newusers(evt),
            "patrol" => self.handle_evt_log_patrol(evt),
            "profile" => self.handle_evt_log_profile(evt),
            "rights" => self.handle_evt_log_rights(evt),
            "thanks" => self.handle_evt_log_thanks(evt),
            "upload" => self.handle_evt_log_upload(evt),
            "usermerge" => self.handle_evt_log_usermerge(evt),
            _ => {
                if log_type == "null" {
                    return;
                }

                let msg = format!(
                    "[log_not_implemented] {}",
                    self.emitter_rgx.plusexclquest_to_url(&evt.dump())
                );

                self.configured_api.emit(msg, true);
            }
        }
    }

    fn handle_evt_log_avatar(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let comment = evt["comment"].to_string();

        let msg = format!(
            r#"[log/avatar] <a href="{}">{}</a> {}"#,

            self.get_user_url(&user),
            user,

            comment
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_block(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            r#"[log/ban] <a href="{}">{}</a> {}"#,

            self.get_user_url(&user),
            user,

            comment
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_delete(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();

        let msg = format!(
            r#"[log/delete] <a href="{}">{}</a> deleted page: <a href="{}">{}</a>"#,

            self.get_user_url(&user),
            user,

            self.get_url(&page),
            page
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_move(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();

        let evt_target = evt["log_params"]["target"].to_string();

        let msg = format!(
            r#"[log/move] <a href="{}">{}</a> moved <a href="{}">{}</a> to <a href="{}">{}</a>"#,

            self.get_user_url(&user),
            user,

            self.get_url(&page),
            page,

            self.get_url(&evt_target),
            evt_target
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_newusers(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();

        let user = evt["user"].to_string();

        let msg = format!(
            r#"[log/newusers] <a href="{}">{}</a> {}"#,

            self.get_user_url(&user),
            user,

            comment
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_approval(&self, evt: &json::JsonValue) {
        let log_type = evt["log_action"].to_string();

        match &*log_type {
            "approve" => self.handle_evt_log_approval_approve(evt),
            "unapprove" => self.handle_evt_log_approval_unapprove(evt),
            _ => {
                if log_type == "null" {
                    return;
                }

                let msg = format!(
                    "[log/approval/not_implemented] {}",
                    self.emitter_rgx.plusexclquest_to_url(&evt.dump())
                );

                self.configured_api.emit(msg, true);
            }
        }
    }

    fn handle_evt_log_approval_approve(&self, evt: &json::JsonValue) {
        let evt_revid = evt["log_params"]["rev_id"].as_u32().unwrap();
        let evt_oldrevid = evt["log_params"]["old_rev_id"].as_u32().unwrap();

        let user = evt["user"].to_string();
        let page = evt["title"].to_string();

        let rev_info: Option<RevInfo> = get_revision_info(page.clone(), evt_revid.to_string());

        let has_rev_info = rev_info.is_some();

        if !has_rev_info {
            eprintln!(
                "Failed to obtain revision information for page='{}', rev_id='{}'",

                page, evt_revid
            );
        }

        // extract user and comment from revision data
        let (rev_by_user, rev_comment, rev_parentid) = {
            if !has_rev_info {
                (String::new(), String::new(), evt_oldrevid.to_string())
            } else {
                let rev_info = rev_info.unwrap();

                (rev_info.0, rev_info.1, rev_info.2)
            }
        };

        let rev_info_msg_user = {
            if !has_rev_info {
                String::new()
            } else {
                format!(
                    r#" by <a href="{}">{}</a> ("{}")"#,

                    self.get_user_url(&rev_by_user),
                    rev_by_user,

                    rev_comment
                )
            }
        };

        let url = format!(
            "https://psychonautwiki.org/w/index.php?title={}&type=revision&diff={:?}&oldid={}",
            self.wrap_urlencode(&MediaWikiEmitter::urlencode(&page)), evt_revid, rev_parentid
        );

        let msg = format!(
            r#"[log/approval] <a href="{}">{}</a> approved <a href="{}">revision {}</a>{} of <a href="{}">{}</a>"#,

            self.get_user_url(&user),
            user,

            url,
            evt_revid,

            rev_info_msg_user,

            self.get_url(&page),
            page
        );

        self.configured_api.emit(msg, true);
    }

    // Currently “unapprove" will unapprove all approved revisions of
    // an article and effectively blank it. Therefore the old revision
    // id will only be used to link to the previously approved revision.
    fn handle_evt_log_approval_unapprove(&self, evt: &json::JsonValue) {
        let evt_oldrevid = evt["log_params"]["old_rev_id"].as_u32().unwrap();

        let user = evt["user"].to_string();
        let page = evt["title"].to_string();

        let rev_info: Option<RevInfo> = get_revision_info(page.clone(), evt_oldrevid.to_string());

        let has_rev_info = rev_info.is_some();

        if !has_rev_info {
            eprintln!(
                "Failed to obtain revision information for page='{}', rev_id='{}'",

                page, evt_oldrevid
            );
        }

        // extract user and comment from revision data
        let (rev_by_user, rev_comment) = {
            if !has_rev_info {
                (String::new(), String::new())
            } else {
                let rev_info = rev_info.unwrap();

                (rev_info.0, rev_info.1)
            }
        };

        let rev_info_msg_user = {
            if !has_rev_info {
                String::new()
            } else {
                format!(
                    r#" by <a href="{}">{}</a> ("{}")"#,

                    self.get_user_url(&rev_by_user),
                    rev_by_user,

                    rev_comment
                )
            }
        };

        let url = format!(
            "https://psychonautwiki.org/w/index.php?title={}&type=revision&oldid={}",
            self.wrap_urlencode(&MediaWikiEmitter::urlencode(&page)), evt_oldrevid
        );

        let msg = format!(
            r#"[log/approval] <a href="{}">{}</a> revoked the approval of <a href="{}">{}</a> (was <a href="{}">revision {}</a>{})"#,

            self.get_user_url(&user),
            user,

            self.get_url(&page),
            page,

            url,
            evt_oldrevid,

            rev_info_msg_user
        );

        self.configured_api.emit(msg, true);
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

        let rev_info: Option<RevInfo> = get_revision_info(page.clone(), evt_curid.to_string());

        let has_rev_info = rev_info.is_some();

        if !has_rev_info {
            eprintln!(
                "Failed to obtain revision information for page='{}', rev_id='{}'",

                page, evt_curid
            );
        }

        // extract user and comment from revision data
        let (rev_by_user, rev_comment) = {
            if !has_rev_info {
                (String::new(), String::new())
            } else {
                let rev_info = rev_info.unwrap();

                (rev_info.0, rev_info.1)
            }
        };

        let rev_info_msg_user = {
            if !has_rev_info {
                String::new()
            } else {
                format!(
                    r#" by <a href="{}">{}</a> ("{}")"#,

                    self.get_user_url(&rev_by_user),
                    rev_by_user,

                    rev_comment
                )
            }
        };

        let url = format!(
            "https://psychonautwiki.org/w/index.php?title={}&type=revision&diff={:?}&oldid={:?}",
            self.wrap_urlencode(&MediaWikiEmitter::urlencode(&page)), evt_curid, evt_previd
        );

        let msg = format!(
            r#"[log/patrol] <a href="{}">{}</a> marked <a href="{}">revision {}</a>{} of <a href="{}">{}</a> patrolled"#,

            self.get_user_url(&user),
            user,

            url,
            evt_curid,

            rev_info_msg_user,

            self.get_url(&page),
            page
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_profile(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();
        let user = evt["user"].to_string();

        let msg = format!(
            r#"[log/profile] <a href="{}">{}</a> {}"#,

            self.get_user_url(&user),
            user,

            comment
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_rights(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            r#"[log/rights] <a href="{}">{}</a> {}"#,

            self.get_user_url(&user),
            user,

            comment
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_thanks(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            "[log/thanks] {}",

            comment
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_upload(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let file = evt["title"].to_string();

        let msg = format!(
            r#"[log/upload] <a href="{}">{}</a> uploaded file: <a href="{}">{}</a>"#,

            self.get_user_url(&user),
            user,

            self.get_url(&file),
            file
        );

        self.configured_api.emit(msg, true);
    }

    fn handle_evt_log_usermerge(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();

        let msg = format!(
            r#"[log/usermerge] <a href="{}">{}</a> {}"#,

            self.get_user_url(&user),
            user,

            MediaWikiEmitter::urldecode(&evt["log_action_comment"].to_string()),
        );

        self.configured_api.emit(msg, true);
    }
}

/*
 * GITHUB CHANGE EVENTS
 */

struct GithubEmitter {
    configured_api: ConfiguredApi
}

impl GithubEmitter {
    fn new() -> GithubEmitter {
        let configured_api = ConfiguredApi::new(&"<b>GitHub</b>", telegram_bot::types::ParseMode::Html);

        GithubEmitter {
            configured_api
        }
    }

    fn handle_evt (&self, delivery: &Delivery) {
        match delivery.payload {
            afterparty::Event::Watch { ref sender, ref repository, .. } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> starred <a href="{}">{}</a>"#,

                    &sender.html_url,
                    sender.login,

                    &repository.html_url,
                    repository.full_name,
                ), false);
            },
            afterparty::Event::CommitComment { ref sender, ref comment, ref repository, .. } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> commented on commit <a href="{}">{}</a>"#,

                    &sender.html_url,
                    sender.login,

                    &comment.html_url,

                    format!(
                        "{}:{}:L{}",

                        repository.full_name,
                        comment.path.clone().unwrap_or("".to_string()),
                        comment.line.clone().unwrap_or(0i64)
                    ),
                ), true);
            }
            afterparty::Event::PullRequest { ref sender, ref action, ref repository, ref pull_request, .. } => {
                // "synchronize" events are less than useless
                if action == "synchronize" {
                    return;
                }

                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> {} pull-request <a href="{}">"{}" (#{})</a> to <a href="{}">{}</a> [<a href="{}">{} commits</a>; <a href="{}">{} changed files (+{}/-{})]</a>; <a href="{}">raw diff</a>]"#,

                    &sender.html_url,
                    sender.login,

                    action,

                    &pull_request.html_url,
                    pull_request.title,
                    pull_request.number,

                    &repository.html_url,
                    repository.full_name,

                    &format!("{}/commits", &pull_request.html_url),
                    pull_request.commits,

                    &format!("{}/files", &pull_request.html_url),
                    pull_request.changed_files,

                    pull_request.additions,
                    pull_request.deletions,

                    &pull_request.diff_url,
                ), false);
            },
            afterparty::Event::PullRequestReview { ref sender, ref action, ref repository, ref pull_request, ref review, .. } => {
                if review.state == "edited" {
                    return;
                }

                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> {} <a href="{}">{}</a> pull-request <a href="{}">"{}" ({}/#{})</a> [<a href="{}">commits</a>; <a href="{}">changed files</a>; <a href="{}">raw diff</a>]"#,

                    &sender.html_url,
                    sender.login,

                    action,

                    &review.html_url,
                    match &*review.state {
                        // these happen either when an approval is
                        // created or dismissed
                        "approved" => "an approval to".to_string(),
                        "dismissed" => "an approval to".to_string(),
                        "commented" => "a comment to".to_string(),
                        "changes_requested" => "a request for changes to".to_string(),
                        _ => review.state.clone()
                    },

                    &pull_request.html_url,
                    pull_request.title,

                    repository.full_name,
                    pull_request.number,

                    &format!("{}/commits", &pull_request.html_url),
                    &format!("{}/files", &pull_request.html_url),
                    &pull_request.diff_url,
                ), false);
            },
            afterparty::Event::Delete { ref sender, ref _ref, ref ref_type, ref repository, .. } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> deleted {} "{}" of <a href="{}">{}</a>"#,

                    &sender.html_url,
                    sender.login,

                    ref_type,
                    _ref,

                    &repository.html_url,
                    repository.full_name,
                ), false);
            },
            afterparty::Event::Release { ref sender, ref action, ref release, ref repository, .. } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> {} release "{}" (tag {}, branch {}{}{}) of <a href="{}">{}</a>:

{}"#,

                    &sender.html_url,
                    sender.login,

                    action,

                    release.name.clone().unwrap_or("?".to_string()),
                    release.tag_name.clone().unwrap_or("?".to_string()),
                    release.target_commitish,

                    match release.draft {
                        true => ", draft",
                        false => "",
                    },

                    match release.prerelease {
                        true => ", prerelease",
                        false => "",
                    },

                    &repository.html_url,
                    repository.full_name,

                    htmlescape_str(release.body.clone().unwrap_or("?".to_string()))
                ), false);
            },
            afterparty::Event::Fork { ref sender, ref repository, ref forkee } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> forked <a href="{}">{}</a> as <a href="{}">{}</a>"#,

                    &sender.html_url,
                    sender.login,

                    &repository.html_url,
                    repository.full_name,

                    &forkee.html_url,
                    forkee.full_name,
                ), false);
            },
            afterparty::Event::IssueComment { ref sender, ref action, ref comment, ref issue, ref repository } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> {} a comment on issue <a href="{}">{}</a> ({:?})"#,

                    &sender.html_url,
                    sender.login,

                    action,

                    {
                        if action == "deleted" {
                            &issue.html_url
                        } else {
                            &comment.html_url
                        }
                    },

                    format!("{}#{}", repository.full_name, issue.number),

                    issue.title
                ), true);
            },
            afterparty::Event::Issues { ref sender, ref action, ref issue, ref repository, .. } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> {} issue <a href="{}">{}</a> ({:?})"#,

                    &sender.html_url,
                    sender.login,

                    action,

                    &issue.html_url,
                    format!("{}#{}", repository.full_name, issue.number),

                    issue.title
                ), true);
            },
            afterparty::Event::Member { ref sender, ref action, ref member, ref repository, .. } => {
                let mut perm_verb = "";
                let mut perm_suffix = "";

                if action == "edited" {
                    perm_verb = "edited the permissions of";
                    perm_suffix = "in";
                } else if action == "added" {
                    perm_verb = "added";
                    perm_suffix = "to";
                } else if action == "deleted" {
                    perm_verb = "removed";
                    perm_suffix = "from";
                }

                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> {} <a href="{}">{}</a> {} <a href="{}">{}</a>"#,

                    &sender.html_url,
                    sender.login,

                    perm_verb,

                    &member.html_url,
                    member.login,

                    perm_suffix,

                    &repository.html_url,
                    repository.full_name,
                ), false);
            },
            afterparty::Event::Membership { ref sender, ref action, ref member, ref team, ref organization, .. } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> was {} <a href="{}">{}/{}</a> by <a href="{}">{}</a>"#,

                    &member.html_url,
                    member.login,

                    {
                        if action == "added" {
                            "added to"
                        } else {
                            "removed from"
                        }
                    },

                    &team.members_url,

                    organization.login,
                    team.name,

                    &sender.html_url,
                    sender.login,
                ), false);
            },
            afterparty::Event::Push { ref forced, ref sender, ref commits, ref compare, ref repository, ref _ref, .. } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> {}pushed <a href="{}">{} commit{}</a> to <a href="{}">{}</a> ({}){}"#,

                    &sender.html_url,
                    sender.login,

                    { if *forced { "force-" } else { "" } },

                    &compare,
                    commits.len(),
                    { if commits.len() == 1 { "" } else { "s" } },

                    &repository.html_url,
                    repository.full_name,

                    _ref,

                    {
                        if commits.len() == 1 {
                            format!(
                                ": {}",

                                // This can potentially trip up the Telegram
                                // HTML parser. That is, encode the string
                                // ensuring Git meta data is not incorrectly
                                // detected as html and thereforce marked
                                // as invalid markup. i.e. "<foo@bar.com>"
                                htmlescape_str(commits[0].message.clone())
                            )
                        } else {
                            "".to_string()
                        }
                    },
                ), true);
            },
            afterparty::Event::Repository { ref sender, ref action, ref repository, .. } => {
                self.configured_api.emit(format!(
                    r#"<a href="{}">{}</a> {} repository <a href="{}">{}</a>"#,

                    &sender.html_url,
                    sender.login,

                    action,

                    &repository.html_url,
                    repository.full_name,
                ), false);
            },
            _ => (),
        }
    }
}

/*
 * JIRA CHANGE EVENTS
 */

// Structs generated by https://transform.now.sh/json-to-rust-serde

#[derive(Serialize, Deserialize)]
struct JiraEventFields {
  issuetype: JiraEventIssuetype,
  project: JiraEventIssuetype,
  priority: JiraEventIssuetype,
  status: JiraEventStatus,
  summary: String,
  creator: JiraEventUser,
  reporter: JiraEventUser,
}

#[derive(Serialize, Deserialize)]
struct JiraEventIssue {
  #[serde(rename = "self")]
  _self: String,
  key: String,
  fields: JiraEventFields,
}

#[derive(Serialize, Deserialize)]
struct JiraEventIssuetype {
  name: String,
}

#[derive(Serialize, Deserialize)]
struct JiraEventStatus {
  name: String,
  #[serde(rename = "statusCategory")]
  status_category: JiraEventIssuetype,
}

#[derive(Serialize, Deserialize)]
struct JiraEventUser {
  #[serde(rename = "accountId")]
  account_id: String,
  #[serde(rename = "displayName")]
  display_name: String,
}

#[derive(Serialize, Deserialize)]
struct JiraEvent<'a> {
  user: JiraEventUser,
  issue: JiraEventIssue,
  #[serde(rename = "webhookEvent")]
  webhook_event: &'a str,
}

enum JiraEventTypes {
    IssueCreated,
    IssueUpdated,
    IssueDeleted
}

struct JiraEmitter {
    configured_api: ConfiguredApi
}

impl JiraEmitter {
    fn new() -> JiraEmitter {
        let configured_api = ConfiguredApi::new(&"<b>Jira</b>", telegram_bot::types::ParseMode::Html);

        JiraEmitter {
            configured_api
        }
    }

    fn handle_evt(&self, event: JiraEvent) {
        let event_type = match event.webhook_event {
            "jira:issue_created" => JiraEventTypes::IssueCreated,
            "jira:issue_updated" => JiraEventTypes::IssueUpdated,
            "jira:issue_deleted" => JiraEventTypes::IssueDeleted,
            _ => { return; }
        };

        self._handle_marked_evt(
            event,
            event_type
        )
    }

    fn _get_verb_from_type(&self, event_type: JiraEventTypes) -> &'static str {
        match event_type {
            JiraEventTypes::IssueUpdated => "updated",
            JiraEventTypes::IssueCreated => "created",
            JiraEventTypes::IssueDeleted => "deleted"
        }
    }

    fn _get_formatted_event(&self, event: JiraEvent, event_type: JiraEventTypes) -> String {
        format!(
            r#"[<i>{}</i> | <i>{}</i>] <a href="https://psychonaut.atlassian.net/people/{}">{}</a> {} {} <a href="https://psychonaut.atlassian.net/browse/{}">{}</a> [{}]: <b>{}</b>"#,

            event.issue.fields.priority.name.to_lowercase(),

            event.issue.fields.status.name.to_lowercase(),

            event.user.account_id,
            event.user.display_name,

            self._get_verb_from_type(event_type),

            event.issue.fields.issuetype.name.to_lowercase(),

            event.issue.key,
            event.issue.key,

            event.issue.fields.project.name,

            event.issue.fields.summary
        )
    }

    fn _handle_marked_evt(&self, event: JiraEvent, event_type: JiraEventTypes) {
        self.configured_api.emit(
            self._get_formatted_event(event, event_type),
            true
        );
    }
}

#[derive(Serialize, Deserialize)]
struct PayPalIPN {
    mc_gross: String,
    protection_eligibility: String,
    payer_id: String,
    payment_date: String,
    payment_status: String,
    charset: String,
    first_name: String,
    mc_fee: String,
    notify_version: String,
    custom: String,
    payer_status: String,
    business: String,
    quantity: String,
    verify_sign: String,
    payer_email: String,
    txn_id: String,
    payment_type: String,
    last_name: String,
    receiver_email: String,
    payment_fee: String,
    shipping_discount: String,
    receiver_id: String,
    insurance_amount: String,
    txn_type: String,
    item_name: String,
    discount: String,
    mc_currency: String,
    item_number: String,
    residence_country: String,
    shipping_method: String,
    transaction_subject: String,
    payment_gross: String,
    ipn_track_id: String,
}

struct PayPalEmitter {
    configured_api: ConfiguredApi
}

impl PayPalEmitter {
    fn new() -> PayPalEmitter {
        let configured_api = ConfiguredApi::new(&"<b>PayPal</b>", telegram_bot::types::ParseMode::Html);

        PayPalEmitter {
            configured_api
        }
    }

    fn handle_evt(&self, event: &PayPalIPN) {
        self.configured_api.emit(
            self._get_formatted_event(event),
            true
        );
    }

    fn _get_formatted_event(&self, event: &PayPalIPN) -> String {
        format!(
            r#"Received <b>{} {}</b> (fee <b>{} {}</b>, gr. <b>{} {}</b>) from <b>{} {}</b> [<b>{}, {}, {}</b>]"#,
            event.mc_currency,
            event.mc_gross.parse::<f64>().unwrap() - event.mc_fee.parse::<f64>().unwrap(),
            event.mc_currency,
            event.mc_fee.parse::<f64>().unwrap(),
            event.mc_currency,
            event.mc_gross.parse::<f64>().unwrap(),
            event.first_name,
            event.last_name,
            if event.payer_status == "verified" { "✓" } else { "✘" },
            event.residence_country,
            event.payer_email
        )
    }
}

/*
 * Main
 */

struct EoP {
    thread_pool: scoped_threadpool::Pool
}

impl EoP {
    fn new() -> EoP {
        EoP {
            thread_pool: Pool::new(4)
        }
    }

    fn init (&mut self) {
        self.thread_pool.scoped(|scoped| {
            scoped.execute(|| {
                EoP::init_mediawiki();
            });

            scoped.execute(|| {
                EoP::init_github();
            });

            scoped.execute(|| {
                EoP::init_jira();
            });

            scoped.execute(|| {
                EoP::init_paypal();
            });
        });
    }

    fn init_mediawiki () {
        let emitter = MediaWikiEmitter::new();

        let socket = match UdpSocket::bind(MEDIAWIKI_ENDPOINT) {
            Ok(socket) => {
                println!("✔ MediaWikiEmitter online. ({})", MEDIAWIKI_ENDPOINT);

                socket
            },
            Err(e) => panic!("✘ MediaWikiEmitter failed to create socket: {}", e)
        };

        let mut buf = [0; 2048];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((amt, _)) => {
                    let instr = std::str::from_utf8(&buf[0..amt]).unwrap_or("");

                    let evt = json::parse(instr);

                    if !evt.is_ok() {
                        continue;
                    }

                    let ref evt = evt.unwrap();

                    emitter.handle_evt(evt);
                },
                Err(e) => println!("couldn't recieve a datagram: {}", e)
            }
        }
    }

    fn init_github () {
        let mut hub = Hub::new();

        hub.handle("*", move |delivery: &Delivery| {
            GithubEmitter::new().handle_evt(delivery);
        });

        let srvc = match Server::http(GITHUB_ENDPOINT) {
            Ok(server) => {
                println!("✔ GithubEmitter online. ({})", GITHUB_ENDPOINT);

                server
            },
            Err(e) => panic!("✘ GithubEmitter failed to create socket: {}", e)
        };

        let _ = srvc.handle(hub);
    }

    fn init_jira () {
        let server = rouille::Server::new(
            JIRA_ENDPOINT,
            move |request| {
                rouille::log(&request, std::io::stdout(), || {
                    router!(request,
                        (POST) (/submit) => {
                            let mut res_data = request.data()
                                                      .expect("Oops, body already retrieved, problem in the server");
                            let mut buf = Vec::new();

                            match res_data.read_to_end(&mut buf) {
                                Ok(_) => (),
                                Err(_) => return rouille::Response::json(&r#"{"ok":false}"#)
                            };

                            let data: JiraEvent = match serde_json::from_slice(&buf) {
                                Ok(parsed_data) => parsed_data,
                                Err(err) => {
                                    println!("{:?}", err);
                                    return rouille::Response::json(&r#"{"ok":false}"#);
                                }
                            };

                            JiraEmitter::new().handle_evt(data);

                            rouille::Response::json(&r#"{"ok":true}"#)
                        },

                        _ => rouille::Response::json(&r#"{"ok":false}"#)
                    )
                })
            }
        );

        match server {
            Ok(server) => {
                println!("✔ JiraEmitter online. ({})", JIRA_ENDPOINT);

                server.run();
            },
            Err(msg) => {
                println!("✘ JiraEmitter failed to create socket: {:?}", msg);
            }
        }
    }

    fn init_paypal () {
        let server = rouille::Server::new(
            PAYPAL_ENDPOINT,
            move |request| {
                rouille::log(&request, std::io::stdout(), || {
                    router!(request,
                        (POST) (/) => {
                            let mut res_data = request.data()
                                                      .expect("?");
                            let mut buf = Vec::new();

                            match res_data.read_to_end(&mut buf) {
                                Ok(_) => (),
                                Err(_) => return rouille::Response::json(&r#"{"ok":false}"#)
                            };

                            verify_paypal_ipn(String::from_utf8(buf.clone()).unwrap());

                            let data: PayPalIPN = match serde_qs::from_str(&*String::from_utf8_lossy(&buf)) {
                                Ok(parsed_data) => parsed_data,
                                Err(_) => return rouille::Response::json(&r#"{"ok":false}"#)
                            };

                            PayPalEmitter::new().handle_evt(&data);

                            rouille::Response::json(&r#"{"ok":true}"#)
                        },

                        _ => rouille::Response::json(&r#"{"ok":false}"#)
                    )
                })
            }
        );

        match server {
            Ok(server) => {
                println!("✔ PayPalEmitter online. ({})", JIRA_ENDPOINT);

                server.run();
            },
            Err(msg) => {
                println!("✘ PayPalEmitter failed to create socket: {:?}", msg);
            }
        }
    }
}

fn main() {
    println!("~~~~~~ PsychonautWiki EoP ~~~~~~");

    let mut eye = EoP::new();

    eye.init();
}
