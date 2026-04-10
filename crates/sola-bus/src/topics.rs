use serde::{Deserialize, Serialize};

use crate::define_topics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub window_count: u32,
}

define_topics! {
    shell {
        ShowSwitcher,
        HideSwitcher,
        ListApps,
        Apps(Vec<App>),
        RaiseApp(String),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topic::Topic;

    #[test]
    fn unit_topic_name() {
        assert_eq!(shell::ShowSwitcher::TOPIC, "shell::ShowSwitcher");
    }

    #[test]
    fn payload_topic_name() {
        assert_eq!(shell::Apps::TOPIC, "shell::Apps");
    }

    #[test]
    fn unit_topic_roundtrip() {
        let msg = shell::ShowSwitcher.to_message();
        assert_eq!(msg.topic, "shell::ShowSwitcher");
        assert!(msg.payload.is_none());
    }

    #[test]
    fn payload_topic_roundtrip() {
        let apps = vec![App {
            id: "zen".into(),
            name: "Browser".into(),
            icon: "globe".into(),
            window_count: 2,
        }];
        let msg = shell::Apps(apps).to_message();
        assert_eq!(msg.topic, "shell::Apps");

        let decoded = shell::Apps::decode(&msg).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].id, "zen");
        assert_eq!(decoded[0].window_count, 2);
    }

    #[test]
    fn raise_app_roundtrip() {
        let msg = shell::RaiseApp("ghostty".into()).to_message();

        match msg.topic.as_str() {
            shell::ShowSwitcher::TOPIC => panic!("wrong topic"),
            shell::RaiseApp::TOPIC => {
                let decoded = shell::RaiseApp::decode(&msg).unwrap();
                assert_eq!(decoded, "ghostty");
            }
            _ => panic!("unmatched topic"),
        }
    }
}
