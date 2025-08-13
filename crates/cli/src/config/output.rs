use std::borrow::Borrow;

use gmsol_sdk::{programs::anchor_lang::prelude::Pubkey, serde::StringPubkey};
use indexmap::IndexMap;
use prettytable::{
    format::{FormatBuilder, LinePosition, LineSeparator, TableFormat},
    row, Cell, Table,
};
use serde::Serialize;
use serde_json::{Map, Value};

/// Output format.
#[derive(clap::ValueEnum, Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    /// Table.
    #[default]
    Table,
    /// JSON.
    Json,
    /// TOML.
    Toml,
}

impl OutputFormat {
    /// Display keyed account.
    pub fn display_keyed_account(
        &self,
        pubkey: &Pubkey,
        account: impl Serialize,
        options: DisplayOptions,
    ) -> eyre::Result<String> {
        let keyed_account = KeyedAccount {
            pubkey: (*pubkey).into(),
            account,
        };
        let Value::Object(map) = serde_json::to_value(keyed_account)? else {
            eyre::bail!("internal: only map-like structures are supported");
        };
        let map = self.project(map, &options);
        match self {
            Self::Table => Self::display_table_one(&map),
            Self::Json => Self::display_json_one(&map),
            Self::Toml => Self::display_toml_one(&map),
        }
    }

    /// Display keyed accounts.
    pub fn display_keyed_accounts(
        &self,
        accounts: impl IntoIterator<Item = (impl Borrow<Pubkey>, impl Serialize)>,
        options: DisplayOptions,
    ) -> eyre::Result<String> {
        let accounts = accounts.into_iter().map(|(pubkey, account)| KeyedAccount {
            pubkey: (*pubkey.borrow()).into(),
            account,
        });
        self.display_many(accounts, options)
    }

    /// Display a list of serializable items.
    pub fn display_many(
        &self,
        items: impl IntoIterator<Item = impl Serialize>,
        options: DisplayOptions,
    ) -> eyre::Result<String> {
        let items = items
            .into_iter()
            .map(|item| {
                let Value::Object(map) = serde_json::to_value(item)? else {
                    eyre::bail!("internal: only map-like structures are supported");
                };
                Ok(self.project(map, &options))
            })
            .collect::<eyre::Result<Vec<_>>>()?;
        match self {
            Self::Table => {
                Self::display_table_many(&items, || options.empty_message.unwrap_or_default())
            }
            Self::Json => Self::display_json_many(&items),
            Self::Toml => Self::display_toml_many(&items),
        }
    }

    fn projection<'a>(&self, options: &'a DisplayOptions) -> Option<&'a IndexMap<String, String>> {
        let proj = options.projection.as_ref()?;
        if options.projection_table_only && !matches!(self, Self::Table) {
            None
        } else {
            Some(proj)
        }
    }

    fn project(&self, mut map: Map<String, Value>, options: &DisplayOptions) -> Map<String, Value> {
        map.append(&mut options.extra.clone());
        if let Some(proj) = self.projection(options) {
            let mut flat = Map::new();
            flatten_json(&map, None, &mut flat);
            proj.iter()
                .map(|(key, name)| (name.clone(), flat.get(key).cloned().unwrap_or(Value::Null)))
                .collect()
        } else {
            map
        }
    }

    fn display_json_many(items: &[Map<String, Value>]) -> eyre::Result<String> {
        Ok(serde_json::to_string_pretty(items)?)
    }

    fn display_toml_many(items: &[Map<String, Value>]) -> eyre::Result<String> {
        Ok(toml::to_string_pretty(items)?)
    }

    fn display_table_many(
        items: &[Map<String, Value>],
        empty_msg: impl FnOnce() -> String,
    ) -> eyre::Result<String> {
        let mut items = items.iter().peekable();
        let Some(first) = items.peek() else {
            return Ok(empty_msg());
        };
        let mut table = Table::new();
        table.set_format(table_format());
        table.set_titles(first.keys().into());

        for item in items {
            table.add_row(item.values().map(json_value_to_cell).collect());
        }

        Ok(table.to_string())
    }

    fn display_json_one(item: &Map<String, Value>) -> eyre::Result<String> {
        Ok(serde_json::to_string_pretty(item)?)
    }

    fn display_toml_one(item: &Map<String, Value>) -> eyre::Result<String> {
        Ok(toml::to_string_pretty(item)?)
    }

    fn display_table_one(item: &Map<String, Value>) -> eyre::Result<String> {
        let mut table = Table::new();
        table.set_format(table_format());
        table.set_titles(row!["Key", "Value"]);

        for (k, v) in item {
            table.add_row(row![k, json_value_to_cell(v)]);
        }

        Ok(table.to_string())
    }

    pub fn display_value_with_label(
        &self,
        label: impl ToString,
        value: impl Serialize,
    ) -> eyre::Result<String> {
        let mut map = Map::new();
        map.insert(label.to_string(), serde_json::to_value(value)?);
        match self {
            Self::Table => Self::display_table_one(&map),
            Self::Json => Self::display_json_one(&map),
            Self::Toml => Self::display_toml_one(&map),
        }
    }

    pub fn display_list_with_header<
        I: IntoIterator<Item = V>,
        V: Serialize,
    >(
        &self,
        items: I,
        header: impl ToString,
    ) -> eyre::Result<String> {
        let header = header.to_string();
        let items = items
            .into_iter()
            .map(|v| serde_json::json!({ header.clone(): v }));
        self.display_many(
            items,
            DisplayOptions::table_projection([(header.as_str(), header.as_str())]),
        )
    }
}

/// Display options.
#[derive(Debug, Clone, Default)]
pub struct DisplayOptions {
    /// An ordered list of keys indicating which parts of the map should be used (i.e., a projection).
    pub projection: Option<IndexMap<String, String>>,
    /// Whether projection should be applied only when the format is `table`.
    pub projection_table_only: bool,
    /// Extra fields.
    pub extra: Map<String, Value>,
    /// Empty message.
    pub empty_message: Option<String>,
}

impl DisplayOptions {
    /// Create a projection for table format only.
    pub fn table_projection(
        keys: impl IntoIterator<Item = (impl ToString, impl ToString)>,
    ) -> Self {
        Self::projection(keys, true)
    }

    /// Create a projection.
    pub fn projection(
        keys: impl IntoIterator<Item = (impl ToString, impl ToString)>,
        projection_table_only: bool,
    ) -> Self {
        Self {
            projection: Some(
                keys.into_iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ),
            projection_table_only,
            extra: Default::default(),
            empty_message: None,
        }
    }

    /// Add extra fields.
    pub fn add_extra(mut self, value: impl Serialize) -> eyre::Result<Self> {
        let Value::Object(mut map) = serde_json::to_value(value)? else {
            eyre::bail!("internal: only map-like structures are supported");
        };
        self.extra.append(&mut map);
        Ok(self)
    }

    /// Set message to display when the table is empty.
    pub fn set_empty_message(mut self, message: impl ToString) -> Self {
        self.empty_message = Some(message.to_string());
        self
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct KeyedAccount<T> {
    pubkey: StringPubkey,
    #[serde(flatten)]
    account: T,
}

fn table_format() -> TableFormat {
    FormatBuilder::new()
        .padding(0, 2)
        .separator(LinePosition::Title, LineSeparator::new('-', '+', '+', '+'))
        .build()
}

fn json_value_to_cell(value: &Value) -> Cell {
    let content = match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "".to_string(),
        other => other.to_string(),
    };

    Cell::new(&content)
}

/// Flatten a nested JSON object into a flat map with `_`-joined keys.
fn flatten_json(map: &Map<String, Value>, prefix: Option<String>, out: &mut Map<String, Value>) {
    for (key, value) in map {
        let full_key = match &prefix {
            Some(p) => format!("{p}.{key}"),
            None => key.to_string(),
        };

        match value {
            Value::Object(obj) => {
                flatten_json(obj, Some(full_key), out);
            }
            _ => {
                out.insert(full_key, value.clone());
            }
        }
    }
}
