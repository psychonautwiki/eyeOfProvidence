use std::str;
use std::net::UdpSocket;

extern crate telegram_bot;
extern crate json;

extern crate afterparty;
use afterparty::{Delivery, Hub};

extern crate hyper;

use hyper::{Client, Server};

use std::io::Read;

extern crate url;
use url::percent_encoding::{
    percent_encode, QUERY_ENCODE_SET
};

extern crate regex;
use regex::Regex;

use std::sync::{Arc, Mutex};

extern crate scoped_threadpool;
use scoped_threadpool::Pool;

extern crate urlshortener;
use urlshortener::{Provider, UrlShortener};

#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

#[macro_use]
extern crate rouille;

const MEDIAWIKI_ENDPOINT: &'static str = "0.0.0.0:3000";
const GITHUB_ENDPOINT: &'static str = "0.0.0.0:4567";
const JIRA_ENDPOINT: &'static str = "0.0.0.0:9293";

const PW_API_URL_PREFIX: &'static str = "https://psychonautwiki.org/w/api.php";

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
struct RevInfo (String, String);

fn get_revision_info(title: String, rev_id: String) -> Option<RevInfo> {
    let title = title;
    let rev_id = rev_id;

    let url = format!(
        "{}?action=query&prop=revisions&titles={}&rvprop=timestamp%7Cuser%7Ccomment%7Ccontent&rvstartid={}&rvendid={}&format=json",

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
            results["comment"].to_string()
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
            percent_rgx: percent_rgx,
            plus_rgx: plus_rgx,
            and_rgx: and_rgx,
            questionmark_rgx: questionmark_rgx,
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
    channel_id: i64,
    name: String,
    parse_mode: Option<telegram_bot::types::ParseMode>,
    url_shortener: UrlShortener
}

impl ConfiguredApi {
    fn new(name: &str, parse_mode: Option<telegram_bot::types::ParseMode>) -> ConfiguredApi {
        let api = telegram_bot::Api::from_env("TELEGRAM_TOKEN").unwrap();

        let url_shortener = UrlShortener::new();

        ConfiguredApi {
            api: api,
            url_shortener: url_shortener,

            channel_id: -1001050593583,
            name: name.to_string(),
            parse_mode: parse_mode
        }
    }

    fn get_short_url (&self, long_url: &str) -> String {
        match self.url_shortener.generate(long_url.to_string(), &Provider::IsGd) {
            Ok(short_url) => short_url,
            Err(_) => long_url.to_string()
        }
    }

    fn emit<T: Into<String>>(&self, msg: T) {
        let _ = self.api.send_message(
            /*chat_id*/
            self.channel_id,

            /*text*/
            format!("⥂ {} ⟹ {}", self.name, msg.into()),

            /*parse_mode*/
            self.parse_mode,

            /*disable_web_page_preview*/
            Some(true),

            /*reply_to_message_id*/
            None,

            /*reply_markup*/
            None
        );
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
        let configured_api = ConfiguredApi::new(&"*MediaWiki*", Some(telegram_bot::types::ParseMode::Markdown));

        let emitter_rgx = EmitterRgx::new();

        MediaWikiEmitter {
            configured_api,
            emitter_rgx
        }
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

            self.configured_api.emit(msg);
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

        self.configured_api.get_short_url(&url)
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

            MediaWikiEmitter::cond_string(evt_is_minor, "*minor* ", ""),
            MediaWikiEmitter::cond_string(evt_is_patrolled, "*patrolled* ", ""),
            MediaWikiEmitter::cond_string(evt_is_bot, "*bot* ", "")
        );

        let msg = format!(
            "{}[{}]({}) edited [{}]({}) {}",

            MediaWikiEmitter::cond_string(
                has_flags,
                &format!("| {}| ", flags),
                ""
            ),

            user,
            self.get_user_url(&user),

            page,
            self.configured_api.get_short_url(&url),

            MediaWikiEmitter::explain_comment(&comment)
        );

        self.configured_api.emit(msg);
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

            MediaWikiEmitter::cond_string(evt_is_minor, "*minor* ", ""),
            MediaWikiEmitter::cond_string(evt_is_patrolled, "*patrolled* ", ""),
            MediaWikiEmitter::cond_string(evt_is_bot, "*bot* ", "")
        );

        let msg = format!(
            "`[`new`]` {}[{}]({}) created page [{}]({}) {}",

            MediaWikiEmitter::cond_string(
                has_flags,
                &format!("| {}| ", flags),
                ""
            ),

            user,
            self.get_user_url(&user),

            page,
            self.configured_api.get_short_url(&url),

            MediaWikiEmitter::explain_comment(&comment)
        );

        self.configured_api.emit(msg);
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

        if log_type == "rights" {
            return self.handle_evt_log_rights(evt);
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
                self.emitter_rgx.plusexclquest_to_url(&evt.dump())
            );

            self.configured_api.emit(msg);
        }
    }

    fn handle_evt_log_avatar(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let comment = evt["comment"].to_string();

        let msg = format!(
            "`[`log/avatar`]` [{}]({}) {}",

            user,
            self.get_user_url(&user),

            comment
        );

        self.configured_api.emit(msg);
    }

    fn handle_evt_log_block(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            "`[`log/ban`]` [{}]({}) {}",

            user,
            self.get_user_url(&user),

            comment
        );

        self.configured_api.emit(msg);
    }

    fn handle_evt_log_delete(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();

        let msg = format!(
            "`[`log/delete`]` [{}]({}) deleted page: [{}]({})",

            user,
            self.get_user_url(&user),

            page,
            self.get_url(&page)
        );

        self.configured_api.emit(msg);
    }

    fn handle_evt_log_move(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let page = evt["title"].to_string();

        let evt_target = evt["log_params"]["target"].to_string();

        let msg = format!(
            "`[`log/move`]` [{}]({}) moved [{}]({}) to [{}]({})",

            user,
            self.get_user_url(&user),

            page,
            self.get_url(&page),

            evt_target,
            self.get_url(&evt_target)
        );

        self.configured_api.emit(msg);
    }

    fn handle_evt_log_newusers(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();

        let user = evt["user"].to_string();

        let msg = format!(
            "`[`log/newusers`]` [{}]({}) {}",

            user,
            self.get_user_url(&user),

            comment
        );

        self.configured_api.emit(msg);
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
                    " by [{}]({}) (\"{}\")",

                    rev_by_user,
                    self.get_user_url(&rev_by_user),

                    rev_comment
                )
            }
        };

        let url = format!(
            "https://psychonautwiki.org/w/index.php?title={}&type=revision&diff={:?}&oldid={:?}",
            self.wrap_urlencode(&MediaWikiEmitter::urlencode(&page)), evt_curid, evt_previd
        );

        let msg = format!(
            "`[`log/patrol`]` [{}]({}) marked [revision {}]({}){} of [{}]({}) patrolled",

            user,
            self.get_user_url(&user),

            evt_curid,
            self.configured_api.get_short_url(&url),

            rev_info_msg_user,

            page,
            self.get_url(&page)
        );

        self.configured_api.emit(msg);
    }

    fn handle_evt_log_profile(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();
        let user = evt["user"].to_string();

        let msg = format!(
            "`[`log/profile`]` [{}]({}) {}",

            user,
            self.get_user_url(&user),

            comment
        );

        self.configured_api.emit(msg);
    }

    fn handle_evt_log_rights(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            "`[`log/rights`]` [{}]({}) {}",

            user,
            self.get_user_url(&user),

            comment
        );

        self.configured_api.emit(msg);
    }

    fn handle_evt_log_thanks(&self, evt: &json::JsonValue) {
        let comment = evt["log_action_comment"].to_string();

        let msg = format!(
            "`[`log/thanks`]` {}",

            comment
        );

        self.configured_api.emit(msg);
    }

    fn handle_evt_log_upload(&self, evt: &json::JsonValue) {
        let user = evt["user"].to_string();
        let file = evt["title"].to_string();

        let msg = format!(
            "`[`log/upload`]` [{}]({}) uploaded file: [{}]({})",

            user,
            self.get_user_url(&user),

            file,
            self.get_url(&file)
        );

        self.configured_api.emit(msg);
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
        let configured_api = ConfiguredApi::new(&"*GitHub*", Some(telegram_bot::types::ParseMode::Markdown));

        GithubEmitter {
            configured_api
        }
    }

    fn handle_evt (&self, delivery: &Delivery) {
        match delivery.payload {
            afterparty::Event::Watch { ref sender, ref repository, .. } => {
                self.configured_api.emit(format!(
                    "[{}]({}) started watching [{}]({})",

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url),

                    repository.full_name,
                    self.configured_api.get_short_url(&repository.html_url)
                ));
            },
            afterparty::Event::CommitComment { ref sender, ref comment, ref repository, .. } => {
                self.configured_api.emit(format!(
                    "[{}]({}) created a comment on [{}]({})",

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url),

                    format!(
                        "{}/{}:{}",

                        repository.full_name,
                        comment.path.clone().unwrap_or("".to_string()),
                        comment.line.clone().unwrap_or("".to_string())
                    ),

                    self.configured_api.get_short_url(&comment.html_url)
                ));
            },
            afterparty::Event::Fork { ref sender, ref repository, ref forkee } => {
                self.configured_api.emit(format!(
                    "[{}]({}) forked [{}]({}) as [{}]({})",

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url),

                    repository.full_name,
                    self.configured_api.get_short_url(&repository.html_url),

                    forkee.full_name,
                    self.configured_api.get_short_url(&forkee.html_url),
                ));
            },
            afterparty::Event::IssueComment { ref sender, ref action, ref comment, ref issue, ref repository } => {
                self.configured_api.emit(format!(
                    "[{}]({}) {} a comment on issue [{}]({}) ({:?})",

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url),

                    action,

                    format!("{}#{}", repository.full_name, issue.number),

                    {
                        if action == "deleted" {
                            self.configured_api.get_short_url(&issue.html_url)
                        } else {
                            self.configured_api.get_short_url(&comment.html_url)
                        }
                    },

                    issue.title
                ));
            },
            afterparty::Event::Issues { ref sender, ref action, ref issue, ref repository } => {
                self.configured_api.emit(format!(
                    "[{}]({}) {} issue [{}]({}) ({:?})",

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url),

                    action,

                    format!("{}#{}", repository.full_name, issue.number),
                    self.configured_api.get_short_url(&issue.html_url),

                    issue.title
                ));
            },
            afterparty::Event::Member { ref sender, ref action, ref member, ref repository } => {
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
                    "[{}]({}) {} [{}]({}) {} [{}]({})",

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url),

                    perm_verb,

                    member.login,
                    self.configured_api.get_short_url(&member.html_url),

                    perm_suffix,

                    repository.full_name,
                    self.configured_api.get_short_url(&repository.html_url),
                ));
            },
            afterparty::Event::Membership { ref sender, ref action, ref member, ref team, ref organization, .. } => {
                self.configured_api.emit(format!(
                    "[{}]({}) was {} [{}/{}]({}) by [{}]({})",

                    member.login,
                    self.configured_api.get_short_url(&member.html_url),

                    {
                        if action == "added" {
                            "added to"
                        } else {
                            "removed from"
                        }
                    },

                    organization.login,

                    team.name,
                    self.configured_api.get_short_url(&team.members_url),

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url)
                ));
            },
            afterparty::Event::Push { ref sender, ref commits, ref compare, ref repository, .. } => {
                self.configured_api.emit(format!(
                    "[{}]({}) pushed [{} commit{}]({}) to [{}]({}){}",

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url),

                    commits.len(),
                    { if commits.len() == 1 { "" } else { "s" } },
                    self.configured_api.get_short_url(&compare),

                    repository.full_name,
                    self.configured_api.get_short_url(&repository.html_url),

                    {
                        if commits.len() == 1 {
                            format!(": {}", commits[0].message)
                        } else {
                            "".to_string()
                        }
                    },
                ));
            },
            afterparty::Event::Repository { ref sender, ref action, ref repository, .. } => {
                self.configured_api.emit(format!(
                    "[{}]({}) {} repository [{}]({})",

                    sender.login,
                    self.configured_api.get_short_url(&sender.html_url),

                    action,

                    repository.full_name,
                    self.configured_api.get_short_url(&repository.html_url)
                ));
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
        let configured_api = ConfiguredApi::new(&"<b>Jira</b>", Some(telegram_bot::types::ParseMode::Html));

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
            self._get_formatted_event(event, event_type)
        );
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
            thread_pool: Pool::new(3)
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
                    let instr = str::from_utf8(&buf[0..amt]).unwrap_or("");

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

        let ge = Arc::new(Mutex::new(GithubEmitter::new()));
        let gex = ge.clone();

        hub.handle("*", move |delivery: &Delivery| {
            gex.lock().unwrap().handle_evt(delivery);
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
        let jira_emitter = Arc::new(Mutex::new(JiraEmitter::new()));

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

                            jira_emitter.lock().unwrap().handle_evt(data);

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
}

fn main() {
    println!("~~~~~~ PsychonautWiki EoP ~~~~~~");

    let mut eye = EoP::new();

    eye.init();
}
