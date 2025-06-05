use super::types::ParsedTwitchMessage;
use chrono::Utc;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct IrcMessage<'a> {
    raw: &'a str,
    tags: Option<&'a str>,
    prefix: Option<&'a str>,
    command: Option<&'a str>,
    params: Vec<&'a str>,
}

impl<'a> IrcMessage<'a> {
    pub fn command(&self) -> Option<&str> {
        self.command
    }

    pub fn prefix(&self) -> Option<&str> {
        self.prefix
    }

    pub fn params(&self) -> &[&str] {
        &self.params
    }

    pub fn parse(line: &'a str) -> Self {
        let mut message = IrcMessage {
            raw: line,
            ..Default::default()
        };
        let mut remainder = line.trim_end_matches(['\r', '\n']);

        if remainder.starts_with('@') {
            if let Some(space_idx) = remainder.find(' ') {
                message.tags = Some(&remainder[1..space_idx]);
                remainder = &remainder[space_idx + 1..];
            } else {
                message.tags = Some(&remainder[1..]);
                return message;
            }
        }
        if remainder.starts_with(':') {
            if let Some(space_idx) = remainder.find(' ') {
                message.prefix = Some(&remainder[1..space_idx]);
                remainder = &remainder[space_idx + 1..];
            } else {
                message.prefix = Some(&remainder[1..]);
                return message;
            }
        }
        if let Some(trail_marker_idx) = remainder.find(" :") {
            let command_and_middle_params_part = &remainder[..trail_marker_idx];
            let trailing_param = &remainder[trail_marker_idx + 2..];
            let mut parts = command_and_middle_params_part.split(' ');
            message.command = parts.next().filter(|s| !s.is_empty());
            for p_str in parts {
                if !p_str.is_empty() {
                    message.params.push(p_str);
                }
            }
            message.params.push(trailing_param);
        } else {
            let mut parts = remainder.split(' ');
            message.command = parts.next().filter(|s| !s.is_empty());
            for p_str in parts {
                if !p_str.is_empty() {
                    message.params.push(p_str);
                }
            }
        }
        message
    }

    pub fn get_tag_value(&self, key_to_find: &str) -> Option<&'a str> {
        self.tags.and_then(|tags_str| {
            tags_str.split(';').find_map(|component| {
                let mut parts = component.splitn(2, '=');
                let key = parts.next()?;
                if key == key_to_find {
                    parts.next().or(Some(""))
                } else {
                    None
                }
            })
        })
    }

    pub fn get_display_name(&self) -> Option<&'a str> {
        self.get_tag_value("display-name")
    }

    pub fn get_prefix_username(&self) -> Option<&'a str> {
        self.prefix.and_then(|p| p.split('!').next())
    }

    pub fn get_privmsg_text_content(&self) -> Option<&'a str> {
        if self.command == Some("PRIVMSG") && self.params.len() > 1 {
            self.params.last().copied()
        } else {
            None
        }
    }

    pub fn to_parsed_twitch_message(&self, channel_name_str: &str) -> Option<ParsedTwitchMessage> {
        if self.command != Some("PRIVMSG") {
            return None;
        }
        let target_channel_in_msg = self.params.first()?.trim_start_matches('#');
        if !target_channel_in_msg.eq_ignore_ascii_case(channel_name_str) {
            return None;
        }

        let initial_text_content = self.get_privmsg_text_content()?.to_string();

        let mut cleaned_text_content = initial_text_content.trim().to_string();

        while let Some(last_char) = cleaned_text_content.chars().last() {
            let char_unicode_val = last_char as u32;

            // Check for various categories of non-content characters
            if last_char.is_control() // Catches Unicode control characters (Cc, Cf categories)
                || last_char.is_whitespace() // Catches broader Unicode whitespace (Zs, Zl, Zp categories)
                || (0xE0000..=0xE007F).contains(&char_unicode_val) // Unicode Tag characters (often used as invisible markers)
                || char_unicode_val == 0x200B // Zero Width Space
                || char_unicode_val == 0xFE0F // Variation Selector 16 (used with emojis, can be appended)
                || char_unicode_val == 0x200C // Zero Width Non-Joiner
                || char_unicode_val == 0x200D
            // Zero Width Joiner
            {
                cleaned_text_content.pop(); // Remove the last character
            } else {
                break;
            }
        }

        let text = cleaned_text_content;

        let sender_username = self
            .get_display_name()
            .or_else(|| self.get_prefix_username())
            .unwrap_or("unknown_user")
            .to_string();
        let sender_user_id = self.get_tag_value("user-id").map(str::to_string);
        let badges_str = self.get_tag_value("badges").map(str::to_string);
        let message_id = self.get_tag_value("id").map(str::to_string);

        let is_moderator = self.get_tag_value("mod") == Some("1")
            || badges_str.as_ref().is_some_and(|b| b.contains("moderator"));
        let is_subscriber = self.get_tag_value("subscriber") == Some("1")
            || self
                .get_tag_value("badges")
                .is_some_and(|b| b.contains("subscriber/"));

        let mut raw_tags_map = HashMap::new();
        if let Some(tags_str) = self.tags {
            for component in tags_str.split(';') {
                let mut parts = component.splitn(2, '=');
                if let Some(key) = parts.next() {
                    raw_tags_map.insert(key.to_string(), parts.next().unwrap_or("").to_string());
                }
            }
        }

        Some(ParsedTwitchMessage {
            channel: channel_name_str.to_string(),
            sender_username,
            sender_user_id,
            text, // Use the cleaned text here
            badges: badges_str,
            is_moderator,
            is_subscriber,
            message_id,
            raw_irc_tags: if raw_tags_map.is_empty() {
                None
            } else {
                Some(raw_tags_map)
            },
            timestamp: Utc::now(),
        })
    }
}
