//! The followed accounts' X timeline.

use std::time::Duration;

use anyhow::Result;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Map, Value, json};

use crate::shown;

/// A post the digest may select from.
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub id: String,
    pub author: String,
    pub text: String,
}

impl Post {
    pub fn to_value(&self) -> Value {
        json!({"id": self.id, "author": self.author, "text": self.text})
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        let field = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| shown!("Invalid pending X post"))
        };
        Ok(Self {
            id: field("id")?,
            author: field("author")?,
            text: field("text")?,
        })
    }
}

/// The X API, by path under `https://api.x.com/2/`.
pub trait Api {
    fn get(&mut self, path: &str, params: &[(String, String)]) -> Result<Value>;
}

/// The X API over HTTPS, as the user whose token it holds.
pub struct Http {
    agent: ureq::Agent,
    token: String,
}

impl Http {
    pub fn new(token: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self { agent, token }
    }
}

impl Api for Http {
    fn get(&mut self, path: &str, params: &[(String, String)]) -> Result<Value> {
        let mut url = format!("https://api.x.com/2/{path}");
        if !params.is_empty() {
            url.push('?');
            url.push_str(
                &url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(params)
                    .finish(),
            );
        }
        let response = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .call();
        let mut response = match response {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(code)) => {
                return Err(shown!(
                    "X returned HTTP {code}. Check user-token access, scopes, credits and rate limits."
                ));
            }
            Err(e) => return Err(e.into()),
        };
        let payload: Value = serde_json::from_str(&response.body_mut().read_to_string()?)?;
        if crate::files::truthy(payload.get("errors")) {
            return Err(shown!(
                "X returned partial errors; keeping the previous digest and cursor"
            ));
        }
        Ok(payload)
    }
}

/// Posts at most 12 000 characters long.
const TEXT_LIMIT: usize = 12_000;

/// The timeline's original posts since `cursor`, or over the past 24 hours without one, newest
/// first, over at most `max_pages` pages of 100; and whether older posts were left out.
pub fn timeline(
    api: &mut dyn Api,
    cursor: Option<&str>,
    now: Timestamp,
    max_pages: i64,
) -> Result<(Vec<Post>, bool)> {
    let me = api.get("users/me", &[])?;
    let me = me
        .pointer("/data/id")
        .and_then(Value::as_str)
        .ok_or_else(|| shown!("X did not return the user's ID"))?
        .to_owned();
    let mut params: Vec<(String, String)> = [
        ("max_results", "100"),
        ("expansions", "author_id"),
        ("tweet.fields", "created_at,note_tweet"),
        ("user.fields", "username"),
        ("exclude", "retweets,replies"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    match cursor {
        Some(cursor) => params.push(("since_id".into(), cursor.into())),
        None => {
            let start = now - SignedDuration::from_hours(24);
            params.push((
                "start_time".into(),
                start.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
            ));
        }
    }
    let mut posts: Map<String, Value> = Map::new();
    let page_path = format!("users/{me}/timelines/reverse_chronological");
    for _ in 0..max_pages {
        let page = api.get(&page_path, &params)?;
        let authors: Map<String, Value> = page
            .pointer("/includes/users")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|u| {
                Some((
                    u.get("id")?.as_str()?.to_owned(),
                    u.get("username")?.clone(),
                ))
            })
            .collect();
        for post in page
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let id = post
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| shown!("X returned a post without ID"))?;
            let author_id = post
                .get("author_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let author = authors
                .get(author_id)
                .and_then(Value::as_str)
                .unwrap_or(author_id);
            let text = post
                .pointer("/note_tweet/text")
                .or_else(|| post.get("text"))
                .and_then(Value::as_str)
                .ok_or_else(|| shown!("X returned a post without text"))?;
            let text: String = text.chars().take(TEXT_LIMIT).collect();
            posts.insert(
                id.to_owned(),
                json!({"id": id, "author": author, "text": text}),
            );
        }
        match page.pointer("/meta/next_token").and_then(Value::as_str) {
            Some(token) if !token.is_empty() => {
                params.retain(|(k, _)| k != "pagination_token");
                params.push(("pagination_token".into(), token.into()));
            }
            _ => return Ok((collect(&posts)?, false)),
        }
    }
    Ok((collect(&posts)?, true))
}

fn collect(posts: &Map<String, Value>) -> Result<Vec<Post>> {
    posts.values().map(Post::from_value).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Pages served in order, with the requests that took them.
    struct Pages(
        pub VecDeque<Value>,
        pub Vec<(String, Vec<(String, String)>)>,
    );

    impl Api for Pages {
        fn get(&mut self, path: &str, params: &[(String, String)]) -> Result<Value> {
            self.1.push((path.to_owned(), params.to_vec()));
            Ok(self.0.pop_front().expect("no more pages"))
        }
    }

    #[test]
    fn paginated_timeline_includes_all_posts() {
        let mut api = Pages(
            [
                json!({"data": {"id": "42"}}),
                json!({"data": [{"id": "124", "author_id": "a", "text": "new"}], "meta": {"next_token": "page2"}}),
                json!({"data": [{"id": "123", "author_id": "a", "text": "older"}]}),
            ]
            .into(),
            Vec::new(),
        );
        let (posts, sampled) = timeline(&mut api, Some("122"), Timestamp::now(), 3).unwrap();
        assert!(!sampled);
        let ids: Vec<_> = posts.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["124", "123"]);
        let (path, params) = &api.1[2];
        assert_eq!(path, "users/42/timelines/reverse_chronological");
        assert!(params.contains(&("since_id".into(), "122".into())));
        assert!(params.contains(&("pagination_token".into(), "page2".into())));
    }

    #[test]
    fn bounded_collection_marks_sampled_coverage() {
        let mut api = Pages(
            [
                json!({"data": {"id": "42"}}),
                json!({"data": [{"id": "123", "author_id": "a", "text": "new"}], "meta": {"next_token": "more"}}),
            ]
            .into(),
            Vec::new(),
        );
        let now: Timestamp = "2026-10-30T18:00:00.5Z".parse().unwrap();
        let (posts, sampled) = timeline(&mut api, None, now, 1).unwrap();
        assert!(sampled);
        assert_eq!(posts[0].id, "123");
        assert!(
            api.1[1]
                .1
                .contains(&("start_time".into(), "2026-10-29T18:00:00Z".into()))
        );
    }

    #[test]
    fn prefers_long_text_and_usernames() {
        let mut api = Pages(
            [
                json!({"data": {"id": "42"}}),
                json!({"data": [{"id": "1", "author_id": "a", "text": "short", "note_tweet": {"text": "long"}}],
                       "includes": {"users": [{"id": "a", "username": "alice"}]}}),
            ]
            .into(),
            Vec::new(),
        );
        let (posts, _) = timeline(&mut api, None, Timestamp::now(), 3).unwrap();
        assert_eq!(
            posts,
            [Post {
                id: "1".into(),
                author: "alice".into(),
                text: "long".into()
            }]
        );
    }
}
