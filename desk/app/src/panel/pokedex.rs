//! The complete shipped roster, overlaid with the server's account-specific discoveries.
use serde_json::Value;
use std::{collections::HashMap, sync::OnceLock};

pub struct Entry {
    pub number: u64,
    pub name: String,
    pub seen: bool,
    pub caught: bool,
    search_name: String,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum Filter {
    #[default]
    All,
    Caught,
    Seen,
    Unseen,
}

#[derive(Default)]
pub struct Browser {
    pub query: String,
    pub filter: Filter,
    pub reset_scroll: bool,
}

impl Browser {
    pub fn matches(&self, entry: &Entry) -> bool {
        let status = match self.filter {
            Filter::All => true,
            Filter::Caught => entry.caught,
            Filter::Seen => entry.seen && !entry.caught,
            Filter::Unseen => !entry.seen,
        };
        if !status {
            return false;
        }
        let query = normalize(&self.query);
        if query.is_empty() {
            return true;
        }
        if let Ok(number) = query.parse::<u64>() {
            return entry.number == number;
        }
        entry.search_name.contains(&query) || normalize(&entry.name).contains(&query)
    }
}

fn normalize(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn roster() -> &'static [(String, u64)] {
    static ROSTER: OnceLock<Vec<(String, u64)>> = OnceLock::new();
    ROSTER.get_or_init(|| {
        let roster: HashMap<String, u64> =
            serde_json::from_str(include_str!("../../../../packages/content/data/dex.json"))
                .expect("shipped Pokédex roster");
        let mut roster: Vec<_> = roster.into_iter().collect();
        roster.sort_by_key(|(_, number)| *number);
        roster
    })
}

pub fn entries(state: &Value) -> Vec<Entry> {
    let discovered: HashMap<u64, &Value> = state["dex"]["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| Some((entry["dex"].as_u64()?, entry)))
        .collect();
    let mut entries: Vec<_> = roster()
        .iter()
        .map(|(species, number)| entry(*number, species, discovered.get(number).copied()))
        .collect();
    // A newer server can introduce species before this desktop build knows their number.
    for (number, known) in discovered {
        if roster()
            .binary_search_by_key(&number, |(_, number)| *number)
            .is_err()
        {
            entries.push(entry(
                number,
                known["species"].as_str().unwrap_or(""),
                Some(known),
            ));
        }
    }
    entries.sort_by_key(|entry| entry.number);
    entries
}

fn entry(number: u64, species: &str, known: Option<&Value>) -> Entry {
    let caught = known.is_some_and(|entry| entry["caught"] == true);
    // Older servers send only encountered species, without an explicit `seen` field.
    let seen = caught || known.is_some_and(|entry| entry["seen"].as_bool().unwrap_or(true));
    Entry {
        number,
        seen,
        caught,
        search_name: normalize(species),
        name: if seen {
            known
                .and_then(|entry| entry["name"].as_str())
                .unwrap_or("Unknown mon")
                .to_string()
        } else {
            "???".into()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_and_discovery_filters_combine_without_revealing_hidden_names() {
        let entries = entries(
            &json!({"dex":{"entries":[{"dex":4,"name":"Charmander","caught":true},{"dex":25,"name":"Pikachu","caught":false}]}}),
        );
        let mut browser = Browser::default();
        let matched = |browser: &Browser| {
            entries
                .iter()
                .filter(|entry| browser.matches(entry))
                .map(|entry| entry.number)
                .collect::<Vec<_>>()
        };
        assert_eq!(matched(&browser).len(), 1025);
        browser.filter = Filter::Caught;
        assert_eq!(matched(&browser), [4]);
        browser.filter = Filter::Seen;
        assert_eq!(matched(&browser), [25]);
        browser.filter = Filter::Unseen;
        assert_eq!(matched(&browser).len(), 1023);
        browser.query = "  BULBASAUR  ".into();
        assert_eq!(matched(&browser), [1]);
        assert_eq!(entries[0].name, "???");
        browser.query = "#0001".into();
        assert_eq!(matched(&browser), [1]);
        browser.query = "Pikachu".into();
        assert!(matched(&browser).is_empty());
        browser.filter = Filter::Seen;
        assert_eq!(matched(&browser), [25]);
        browser.filter = Filter::All;
        browser.query = "Mr. Mime".into();
        assert_eq!(matched(&browser), [122]);
        browser.query = "farfetch'd".into();
        assert_eq!(matched(&browser), [83]);
        browser.query = "no-such-mon".into();
        assert!(matched(&browser).is_empty());
    }

    #[test]
    fn full_roster_includes_unseen_without_inflating_discoveries() {
        let entries = entries(
            &json!({"dex":{"entries":[{"species":"charmander","dex":4,"name":"Charmander","caught":true},{"species":"pikachu","dex":25,"name":"Pikachu","caught":false}]}}),
        );
        assert_eq!(entries.len(), 1025);
        assert_eq!(entries.iter().filter(|entry| entry.seen).count(), 2);
        assert_eq!(entries.iter().filter(|entry| entry.caught).count(), 1);
        assert!(entries
            .windows(2)
            .all(|pair| pair[0].number < pair[1].number));
        assert_eq!(entries[0].name, "???");
        assert!(!entries[0].seen);
        assert_eq!(entries[3].name, "Charmander");
        assert_eq!(entries[24].name, "Pikachu");
        assert!(entries[24].seen && !entries[24].caught);
        assert_eq!(entries.last().unwrap().number, 1025);
    }

    #[test]
    fn handles_explicit_unseen_and_newer_server_species() {
        let entries = entries(
            &json!({"dex":{"entries":[{"dex":1,"name":"Bulbasaur","seen":false,"caught":false},{"dex":2,"name":"Ivysaur","seen":false,"caught":true},{"dex":2000,"name":"Future mon","caught":false},{"dex":1999,"name":"Other mon","caught":false}]}}),
        );
        assert_eq!(entries.len(), 1027);
        assert_eq!(entries[0].name, "???");
        assert!(entries[1].seen && entries[1].caught);
        assert_eq!(entries[1025].number, 1999);
        assert_eq!(entries[1026].name, "Future mon");
    }
}
