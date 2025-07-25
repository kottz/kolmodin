use super::types::ParsedTwitchMessage;
use chrono::Utc;
use std::collections::HashMap;
use thiserror::Error;

// IRC Commands
pub const CMD_PING: &str = "PING";
pub const CMD_PONG: &str = "PONG";
pub const CMD_JOIN: &str = "JOIN";
pub const CMD_PRIVMSG: &str = "PRIVMSG";
pub const CMD_PASS: &str = "PASS";
pub const CMD_NICK: &str = "NICK";
pub const CMD_NOTICE: &str = "NOTICE";
pub const CMD_RECONNECT: &str = "RECONNECT";
pub const CMD_CAP: &str = "CAP";

// IRC Replies
pub const RPL_WELCOME: &str = "001";

// IRC Keywords
pub const IRC_ACK: &str = "ACK";
pub const IRC_NAK: &str = "NAK";
// Special IRC strings
pub const IRC_WELCOME_TEXT: &str = ":Welcome";
pub const HEALTH_CHECK_PING: &str = "PING :health-check";

// Twitch capabilities
pub const TWITCH_CAPABILITIES: &str =
    "CAP REQ :twitch.tv/membership twitch.tv/tags twitch.tv/commands";

// Authentication error messages
pub const AUTH_ERROR_LOGIN_FAILED: &str = "Login authentication failed";
pub const AUTH_ERROR_IMPROPERLY_FORMATTED: &str = "Improperly formatted auth";
pub const AUTH_ERROR_INVALID_NICK: &str = "Invalid NICK";

#[derive(Error, Debug, Clone, PartialEq)]
pub enum IrcParseError {
    #[error("Input line is empty or only whitespace")]
    EmptyInput,
    #[error(
        "Tags section is malformed (starts with '@' but has no following space before command/prefix)"
    )]
    MalformedTags,
    #[error(
        "Prefix section is malformed (starts with ':' but has no following space before command)"
    )]
    MalformedPrefix,
    #[error("No command found in the message")]
    MissingCommand,
}

#[derive(Debug, Default, PartialEq)]
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

    pub fn parse(line: &'a str) -> Result<Self, IrcParseError> {
        // Initial trim and empty check
        let mut remainder = line.trim_end_matches(['\r', '\n']);

        // Check for empty input
        if remainder.trim().is_empty() {
            return Err(IrcParseError::EmptyInput);
        }

        let mut message = IrcMessage {
            raw: line,
            ..Default::default()
        };

        // Parse tags section
        if remainder.starts_with('@') {
            if let Some(space_idx) = remainder.find(' ') {
                message.tags = Some(&remainder[1..space_idx]);
                remainder = &remainder[space_idx + 1..];
            } else {
                // Line contains only tags, which is invalid IRC
                return Err(IrcParseError::MalformedTags);
            }
        }

        // Parse prefix section
        if remainder.starts_with(':') {
            if let Some(space_idx) = remainder.find(' ') {
                message.prefix = Some(&remainder[1..space_idx]);
                remainder = &remainder[space_idx + 1..];
            } else {
                // Line contains only prefix, which is invalid IRC
                return Err(IrcParseError::MalformedPrefix);
            }
        }

        // Parse command and parameters
        if let Some(trail_marker_idx) = remainder.find(" :") {
            let command_and_middle_params_part = &remainder[..trail_marker_idx];
            let trailing_param = &remainder[trail_marker_idx + 2..];
            let mut parts = command_and_middle_params_part.split(' ');

            // Extract command
            message.command = parts.next().filter(|s| !s.is_empty());
            if message.command.is_none() {
                return Err(IrcParseError::MissingCommand);
            }

            // Extract middle parameters
            for p_str in parts {
                if !p_str.is_empty() {
                    message.params.push(p_str);
                }
            }
            message.params.push(trailing_param);
        } else {
            let mut parts = remainder.split(' ');

            // Extract command
            message.command = parts.next().filter(|s| !s.is_empty());
            if message.command.is_none() {
                return Err(IrcParseError::MissingCommand);
            }

            // Extract parameters
            for p_str in parts {
                if !p_str.is_empty() {
                    message.params.push(p_str);
                }
            }
        }

        Ok(message)
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
        if self.command == Some(CMD_PRIVMSG) && self.params.len() > 1 {
            self.params.last().copied()
        } else {
            None
        }
    }

    pub fn to_parsed_twitch_message(&self, channel_name_str: &str) -> Option<ParsedTwitchMessage> {
        if self.command != Some(CMD_PRIVMSG) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        assert_eq!(IrcMessage::parse(""), Err(IrcParseError::EmptyInput));
        assert_eq!(IrcMessage::parse("   "), Err(IrcParseError::EmptyInput));
        assert_eq!(IrcMessage::parse("\r\n"), Err(IrcParseError::EmptyInput));
    }

    #[test]
    fn test_malformed_tags() {
        assert_eq!(
            IrcMessage::parse("@tag1=value1"),
            Err(IrcParseError::MalformedTags)
        );
        assert_eq!(
            IrcMessage::parse("@tag1=value1COMMAND"),
            Err(IrcParseError::MalformedTags)
        );
    }

    #[test]
    fn test_malformed_prefix() {
        assert_eq!(
            IrcMessage::parse(":nick!user@host"),
            Err(IrcParseError::MalformedPrefix)
        );
        assert_eq!(
            IrcMessage::parse(":nick!user@hostCOMMAND"),
            Err(IrcParseError::MalformedPrefix)
        );
    }

    #[test]
    fn test_missing_command() {
        assert_eq!(
            IrcMessage::parse("@tag1=value1 "),
            Err(IrcParseError::MissingCommand)
        );
        assert_eq!(
            IrcMessage::parse(":nick!user@host "),
            Err(IrcParseError::MissingCommand)
        );
    }

    #[test]
    fn test_valid_messages() {
        assert!(IrcMessage::parse("COMMAND").is_ok());
        assert!(IrcMessage::parse("COMMAND param1").is_ok());
        assert!(IrcMessage::parse("COMMAND param1 :trailing param").is_ok());
        assert!(IrcMessage::parse("COMMAND :").is_ok());
        assert!(IrcMessage::parse(":tmi.twitch.tv PONG tmi.twitch.tv :health-check").is_ok());
        assert!(IrcMessage::parse("@badge-info=;badges=;color=;display-name=TestUser;emotes=;first-msg=0;flags=;id=abc;mod=0;returning-chatter=0;room-id=123;subscriber=0;turbo=0;user-id=456;user-type= :testuser!testuser@testuser.tmi.twitch.tv PRIVMSG #channel :Hello World").is_ok());
    }
}
