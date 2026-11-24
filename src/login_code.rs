//! Habbo login-code parsing (`<server>.<uuid>.<v4>[.<username>]` or a `habbo://` URL).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginCode {
    pub sso_ticket: String,
    pub server_id: String,
    pub server_url: String,
    pub username: String,
}

const SERVERS: &[(&str, &str)] = &[
    ("hhus", "www.habbo.com"),
    ("hhfr", "www.habbo.fr"),
    ("hhes", "www.habbo.es"),
    ("hhbr", "www.habbo.com.br"),
    ("hhfi", "www.habbo.fi"),
    ("hhtr", "www.habbo.com.tr"),
    ("hhde", "www.habbo.de"),
    ("hhnl", "www.habbo.nl"),
    ("hhit", "www.habbo.it"),
    ("local", "localhost:s3dcom:3000"),
    ("hhs1", "s1.varoke.net"),
    ("hhs2", "sandbox.habbo.com"),
    ("duke", "duke.varoke.net"),
    ("d63", "d63.varoke.net"),
    ("dev", "dev.varoke.net"),
    ("hhxd", "habbox.varoke.net"),
    ("hhxp", "www.habbox.game"),
];

fn server_url(id: &str) -> Option<&'static str> {
    SERVERS
        .iter()
        .find(|(sid, _)| *sid == id)
        .map(|(_, url)| *url)
}

impl LoginCode {
    /// Accepts either a raw code or `habbo://hab?server=hhes&token=...&...` protocol URL.
    pub fn parse(raw: &str) -> Option<Self> {
        let mut text = raw.trim().to_string();
        if text.starts_with("habbo://") && text.contains("server=") {
            let start = text.find("?server=").map(|i| i + 8)?;
            text = text[start..].to_string();
            text = text.replacen("&token=", ".", 1);
            // strip anything after the token block (e.g. "&foo=bar")
            if let Some(pos) = text.find('&') {
                text.truncate(pos);
            }
        }

        let parts: Vec<&str> = text.split('.').collect();
        // Two accepted shapes:
        //   hhus.84566da9-...-63384688            (current website login code)
        //   hhes.<uuid>.V4[.<username>]           (older / protocol form)
        if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
            return None;
        }
        let server_id = parts[0].to_string();
        let server_url = server_url(&server_id)?.to_string();
        let (sso_ticket, username) = if parts.len() >= 3 {
            (
                format!("{}.{}", parts[1], parts[2]),
                parts.get(3).map(|s| s.to_string()).unwrap_or_default(),
            )
        } else {
            (parts[1].to_string(), String::new())
        };
        Some(LoginCode {
            sso_ticket,
            server_id,
            server_url,
            username,
        })
    }

    /// Short hotel tag shown in the UI, e.g. `hhes` -> `HES`.
    pub fn hotel_tag(&self) -> String {
        self.server_id
            .trim_start_matches("hh")
            .to_uppercase()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ticket_with_username() {
        let c = LoginCode::parse("hhes.11111111-1111-1111-1111-111111111111-11111111.V4.Lilith")
            .unwrap();
        assert_eq!(c.server_id, "hhes");
        assert_eq!(
            c.sso_ticket,
            "11111111-1111-1111-1111-111111111111-11111111.V4"
        );
        assert_eq!(c.server_url, "www.habbo.es");
        assert_eq!(c.username, "Lilith");
        assert_eq!(c.hotel_tag(), "ES");
    }

    #[test]
    fn parses_protocol_url() {
        let c = LoginCode::parse("habbo://hab?server=hhus&token=abc.def").unwrap();
        assert_eq!(c.server_id, "hhus");
        assert_eq!(c.sso_ticket, "abc.def");
        assert_eq!(c.server_url, "www.habbo.com");
    }

    #[test]
    fn parses_website_login_code() {
        let c = LoginCode::parse("hhus.84566da9-8d3a-4271-95e6-e20a013ea976-63384688").unwrap();
        assert_eq!(c.server_id, "hhus");
        assert_eq!(
            c.sso_ticket,
            "84566da9-8d3a-4271-95e6-e20a013ea976-63384688"
        );
        assert_eq!(c.server_url, "www.habbo.com");
        assert_eq!(c.username, "");
    }

    #[test]
    fn rejects_unknown_server() {
        assert!(LoginCode::parse("zzzz.abc.def.V4").is_none());
        assert!(LoginCode::parse("garbage").is_none());
    }
}
