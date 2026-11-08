//! Blocking provider client. Call on a worker, never on an audio or UI thread.
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use std::{io::Read, time::Duration};

const MAX_AUDIO_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum Error {
    Credentials,
    Permission,
    Quota,
    RateLimit,
    Subscription,
    UnusualActivity,
    Connection,
    Provider,
    InvalidAudio,
    InvalidInput,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Voice {
    pub voice_id: String,
    pub name: String,
}

#[derive(Deserialize)]
struct Voices {
    voices: Vec<Voice>,
    has_more: bool,
    next_page_token: Option<String>,
}

pub struct SpeechClient {
    http: Client,
    base: String,
}

impl SpeechClient {
    pub fn new() -> Result<Self, Error> {
        Self::at("https://api.elevenlabs.io")
    }

    fn at(base: &str) -> Result<Self, Error> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Error::Connection)?;
        Ok(Self {
            http,
            base: base.into(),
        })
    }

    fn checked(response: Response) -> Result<Response, Error> {
        let status = response.status().as_u16();
        if (200..=299).contains(&status) {
            return Ok(response);
        }
        // Only recognize provider codes; never expose its raw response or request secrets.
        #[derive(Deserialize, Default)]
        struct Detail {
            status: Option<String>,
            code: Option<String>,
            #[serde(rename = "type")]
            kind: Option<String>,
        }
        #[derive(Deserialize)]
        struct Failure {
            #[serde(default)]
            detail: Detail,
            #[serde(flatten)]
            fields: Detail,
        }
        let failure = serde_json::from_reader::<_, Failure>(response.take(16 * 1024)).ok();
        let code = failure.as_ref().and_then(|f| {
            f.detail
                .code
                .as_deref()
                .or(f.fields.code.as_deref())
                .or(f.detail.status.as_deref())
                .or(f.detail.kind.as_deref())
                .or(f.fields.kind.as_deref())
        });
        Err(match code {
            Some("quota_exceeded" | "insufficient_credits") => Error::Quota,
            Some("payment_required" | "subscription_required" | "paid_plan_required") => {
                Error::Subscription
            }
            Some("detected_unusual_activity" | "unusual_activity") => Error::UnusualActivity,
            Some(
                "too_many_concurrent_requests"
                | "rate_limit_exceeded"
                | "concurrent_limit_exceeded"
                | "rate_limit_error",
            ) => Error::RateLimit,
            Some("system_busy") => Error::Provider,
            Some("invalid_api_key" | "authentication_error") => Error::Credentials,
            Some("missing_permissions" | "authorization_error") => Error::Permission,
            _ => match status {
                401 => Error::Credentials,
                403 => Error::Permission,
                402 => Error::Subscription,
                429 => Error::RateLimit,
                400 | 422 => Error::InvalidInput,
                _ => Error::Provider,
            },
        })
    }

    pub fn voices(&self, key: &str) -> Result<Vec<Voice>, Error> {
        let mut result = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut request = self
                .http
                .get(format!("{}/v2/voices", self.base))
                .header("xi-api-key", key)
                .query(&[("page_size", "100")]);
            if let Some(token) = &token {
                request = request.query(&[("next_page_token", token)]);
            }
            let page: Voices = Self::checked(request.send().map_err(|_| Error::Connection)?)?
                .json()
                .map_err(|_| Error::Provider)?;
            result.extend(page.voices);
            if !page.has_more {
                return Ok(result);
            }
            let next = page.next_page_token.ok_or(Error::Provider)?;
            if token.as_ref() == Some(&next) || result.len() > 10_000 {
                return Err(Error::Provider);
            }
            token = Some(next);
        }
    }

    pub fn generate(&self, key: &str, voice: &str, text: &str) -> Result<Vec<u8>, Error> {
        if text.trim().is_empty()
            || text.chars().count() > 5_000
            || voice.is_empty()
            || !voice
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::InvalidInput);
        }
        #[derive(Serialize)]
        struct Body<'a> {
            text: &'a str,
            model_id: &'a str,
        }
        let response = self
            .http
            .post(format!("{}/v1/text-to-speech/{voice}", self.base))
            .query(&[("output_format", "mp3_44100_128")])
            .header("xi-api-key", key)
            .json(&Body {
                text,
                model_id: "eleven_multilingual_v2",
            })
            .send()
            .map_err(|_| Error::Connection)?;
        let response = Self::checked(response)?;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_AUDIO_BYTES)
        {
            return Err(Error::InvalidAudio);
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_AUDIO_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Connection)?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_AUDIO_BYTES {
            return Err(Error::InvalidAudio);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn server(status: &str, body: &str) -> (SpeechClient, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client =
            SpeechClient::at(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = [0; 8192];
            let len = stream.read(&mut bytes).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&bytes[..len]).into_owned()
        });
        (client, handle)
    }

    #[test]
    fn provider_errors_are_sanitized() {
        for (status, expected) in [
            ("401 Unauthorized", Error::Credentials),
            ("403 Forbidden", Error::Permission),
            ("429 Too Many Requests", Error::RateLimit),
            ("500 Error", Error::Provider),
        ] {
            let (client, server) = server(status, "secret provider detail");
            assert_eq!(client.generate("private-key", "voice", "Go"), Err(expected));
            server.join().unwrap();
        }
    }

    #[test]
    fn provider_details_distinguish_credits_plan_and_temporary_limits() {
        for (status, body, expected) in [
            (
                "401 Unauthorized",
                r#"{"detail":{"status":"quota_exceeded"}}"#,
                Error::Quota,
            ),
            (
                "402 Payment Required",
                r#"{"detail":{"status":"payment_required"}}"#,
                Error::Subscription,
            ),
            (
                "403 Forbidden",
                r#"{"detail":{"status":"detected_unusual_activity"}}"#,
                Error::UnusualActivity,
            ),
            (
                "429 Too Many Requests",
                r#"{"detail":{"status":"too_many_concurrent_requests"}}"#,
                Error::RateLimit,
            ),
            (
                "429 Too Many Requests",
                r#"{"detail":{"status":"system_busy"}}"#,
                Error::Provider,
            ),
            (
                "402 Payment Required",
                r#"{"type":"payment_required","code":"insufficient_credits"}"#,
                Error::Quota,
            ),
            (
                "429 Too Many Requests",
                r#"{"detail":{"code":"concurrent_limit_exceeded","type":"rate_limit_error"}}"#,
                Error::RateLimit,
            ),
            ("402 Payment Required", "invalid json", Error::Subscription),
            (
                "401 Unauthorized",
                r#"{"detail":"private provider message"}"#,
                Error::Credentials,
            ),
        ] {
            let (client, server) = server(status, body);
            assert_eq!(client.generate("private-key", "voice", "Go"), Err(expected));
            server.join().unwrap();
        }
    }

    #[test]
    fn speech_is_downloaded_and_voices_are_parsed() {
        let (client, server) = server("200 OK", "audio");
        assert_eq!(client.generate("key", "voice", "Go").unwrap(), b"audio");
        assert!(
            server
                .join()
                .unwrap()
                .contains("output_format=mp3_44100_128")
        );
        let (client, server) = self::server(
            "200 OK",
            r#"{"voices":[{"voice_id":"v","name":"Operator"}],"has_more":false}"#,
        );
        assert_eq!(client.voices("key").unwrap()[0].voice_id, "v");
        server.join().unwrap();
    }

    #[test]
    fn empty_audio_and_invalid_input_are_rejected_without_retries() {
        let (client, server) = server("200 OK", "");
        assert_eq!(
            client.generate("key", "voice", "Go"),
            Err(Error::InvalidAudio)
        );
        server.join().unwrap();
        let client = SpeechClient::new().unwrap();
        assert_eq!(
            client.generate("key", "../voice", "Go"),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            client.generate("key", "voice", "  "),
            Err(Error::InvalidInput)
        );
    }

    #[test]
    fn a_request_timeout_returns_a_connection_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = SpeechClient {
            http: Client::builder()
                .timeout(Duration::from_millis(30))
                .build()
                .unwrap(),
            base: format!("http://{}", listener.local_addr().unwrap()),
        };
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(100));
        });
        assert_eq!(
            client.generate("key", "voice", "Go"),
            Err(Error::Connection)
        );
        server.join().unwrap();
    }
}
