//! Deliberately bounded OData support. Unsupported queries fail explicitly.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

pub type QueryResult<T> = std::result::Result<T, String>;

pub struct Query(BTreeMap<String, String>);

impl Query {
    pub fn page(
        &self,
        request: &url::Url,
        endpoint: &str,
        mut rows: Vec<Value>,
        page_size: usize,
    ) -> QueryResult<Value> {
        if let Some(top) = self.0.get("$top") {
            rows.truncate(
                top.parse()
                    .map_err(|_| "$top must be a nonnegative integer.")?,
            );
        }
        let start = self
            .0
            .get("$skiptoken")
            .map(|s| {
                s.strip_prefix("Paged=TRUE&p_ID=")
                    .ok_or("Unsupported simulator continuation token.")?
                    .parse::<usize>()
                    .map_err(|_| "Invalid simulator continuation token.")
            })
            .transpose()?
            .unwrap_or(0);
        let total = rows.len();
        let page: Vec<_> = rows.into_iter().skip(start).take(page_size).collect();
        let next_start = start.saturating_add(page.len());
        let mut body = serde_json::json!({"value": page});
        if next_start < total {
            let mut next = url::Url::parse(&format!("{endpoint}{}", request.path()))
                .map_err(|e| e.to_string())?;
            for (key, value) in request.query_pairs().filter(|(key, _)| key != "$skiptoken") {
                next.query_pairs_mut().append_pair(&key, &value);
            }
            next.query_pairs_mut()
                .append_pair("$skiptoken", &format!("Paged=TRUE&p_ID={next_start}"));
            body["odata.nextLink"] = Value::String(next.into());
        }
        Ok(body)
    }
    pub fn filter(&self, rows: &mut Vec<Value>, columns: &[&str]) -> QueryResult<()> {
        if let Some(filter) = self.0.get("$filter") {
            let tokens = tokens(filter)?;
            let mut parser = FilterParser {
                tokens: &tokens,
                position: 0,
                columns,
            };
            let expression = parser.or(0)?;
            if parser.position != tokens.len() {
                return Err("Unexpected filter token.".into());
            }
            rows.retain(|row| expression.matches(row));
        }
        Ok(())
    }
    pub fn parse(url: &url::Url, allowed: &[&str]) -> QueryResult<Self> {
        let mut params = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if !allowed.contains(&key.as_ref()) {
                return Err(format!("Unsupported query parameter: {key}"));
            }
            if params.insert(key.to_string(), value.to_string()).is_some() {
                return Err(format!("Duplicate query parameter: {key}"));
            }
        }
        Ok(Self(params))
    }

    pub fn projection(&self, columns: &[&str], expandable: &[&str]) -> QueryResult<Projection> {
        let expansions = self
            .0
            .get("$expand")
            .map(|s| csv(s))
            .transpose()?
            .unwrap_or_default();
        for expansion in &expansions {
            if !expandable.contains(&expansion.as_str()) {
                return Err(format!("Unsupported expansion: {expansion}"));
            }
        }
        let fields = match self.0.get("$select").map(String::as_str) {
            None | Some("*") => columns
                .iter()
                .filter(|field| {
                    field
                        .split_once('/')
                        .is_none_or(|(root, _)| expansions.iter().any(|e| e == root))
                })
                .map(|s| s.to_string())
                .collect(),
            Some(select) => csv(select)?,
        };
        for field in &fields {
            if !columns.contains(&field.as_str()) {
                return Err(format!("Unknown selected property: {field}"));
            }
            if let Some((root, _)) = field.split_once('/')
                && !expansions.iter().any(|e| e == root)
            {
                return Err(format!("The simulator requires $expand={root} for {field}"));
            }
        }
        Ok(Projection(fields))
    }
}

pub struct Projection(Vec<String>);

impl Projection {
    pub fn apply(&self, input: &Value) -> Value {
        let mut output = Map::new();
        for field in &self.0 {
            if let Some((root, child)) = field.split_once('/') {
                output
                    .entry(root)
                    .or_insert_with(|| Value::Object(Map::new()))[child] =
                    input[root][child].clone();
            } else {
                output.insert(field.clone(), input[field].clone());
            }
        }
        Value::Object(output)
    }
}

fn csv(value: &str) -> QueryResult<Vec<String>> {
    let fields: Vec<_> = value.split(',').map(|s| s.trim().to_string()).collect();
    if fields.iter().any(String::is_empty) {
        return Err("Empty property name.".into());
    }
    Ok(fields)
}

#[derive(PartialEq)]
enum Token {
    Word(String),
    Text(String),
    Open,
    Close,
}

fn tokens(input: &str) -> QueryResult<Vec<Token>> {
    let mut input = input.chars().peekable();
    let mut result = Vec::new();
    while let Some(c) = input.next() {
        match c {
            c if c.is_whitespace() => {}
            '(' => result.push(Token::Open),
            ')' => result.push(Token::Close),
            '\'' => {
                let mut text = String::new();
                loop {
                    match input.next() {
                        Some('\'') if input.peek() == Some(&'\'') => {
                            input.next();
                            text.push('\'');
                        }
                        Some('\'') => break,
                        Some(c) => text.push(c),
                        None => return Err("Unterminated filter string.".into()),
                    }
                }
                result.push(Token::Text(text));
            }
            _ => {
                let mut word = c.to_string();
                while input
                    .peek()
                    .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | ')' | '\''))
                {
                    word.push(input.next().unwrap());
                }
                result.push(Token::Word(word));
            }
        }
    }
    Ok(result)
}

enum Filter {
    Equal(String, Value),
    And(Box<Filter>, Box<Filter>),
    Or(Box<Filter>, Box<Filter>),
}

impl Filter {
    fn matches(&self, row: &Value) -> bool {
        match self {
            Self::Equal(key, value) => row[key] == *value,
            Self::And(a, b) => a.matches(row) && b.matches(row),
            Self::Or(a, b) => a.matches(row) || b.matches(row),
        }
    }
}

struct FilterParser<'a> {
    tokens: &'a [Token],
    position: usize,
    columns: &'a [&'a str],
}

impl FilterParser<'_> {
    fn consume(&mut self, word: &str) -> bool {
        if self.tokens.get(self.position) == Some(&Token::Word(word.into())) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self, depth: usize) -> QueryResult<Filter> {
        let mut expression = self.and(depth)?;
        while self.consume("or") {
            expression = Filter::Or(Box::new(expression), Box::new(self.and(depth)?));
        }
        Ok(expression)
    }

    fn and(&mut self, depth: usize) -> QueryResult<Filter> {
        let mut expression = self.atom(depth)?;
        while self.consume("and") {
            expression = Filter::And(Box::new(expression), Box::new(self.atom(depth)?));
        }
        Ok(expression)
    }

    fn atom(&mut self, depth: usize) -> QueryResult<Filter> {
        if depth > 32 {
            return Err("Filter nesting is too deep.".into());
        }
        if self.tokens.get(self.position) == Some(&Token::Open) {
            self.position += 1;
            let expression = self.or(depth + 1)?;
            if self.tokens.get(self.position) != Some(&Token::Close) {
                return Err("Missing closing filter parenthesis.".into());
            }
            self.position += 1;
            return Ok(expression);
        }
        let Some(Token::Word(field)) = self.tokens.get(self.position) else {
            return Err("Expected filter property.".into());
        };
        let field = field.clone();
        if !self.columns.contains(&field.as_str()) {
            return Err(format!("Unknown filter property: {field}"));
        }
        if field.contains('/') {
            return Err("The simulator supports filters on scalar properties only.".into());
        }
        self.position += 1;
        if !self.consume("eq") {
            return Err(
                "The simulator supports only eq comparisons, and/or, and parentheses.".into(),
            );
        }
        let value = match (field.as_str(), self.tokens.get(self.position)) {
            ("Hidden", Some(Token::Word(s))) if s == "true" || s == "false" => {
                Value::Bool(s == "true")
            }
            ("BaseTemplate", Some(Token::Word(s))) => Value::from(
                s.parse::<u32>()
                    .map_err(|_| "BaseTemplate must be an integer.")?,
            ),
            ("Hidden" | "BaseTemplate", _) => {
                return Err(format!("Wrong filter value type for {field}"));
            }
            (_, Some(Token::Text(s))) => Value::String(s.clone()),
            _ => return Err(format!("Filter value for {field} must be a quoted string.")),
        };
        self.position += 1;
        Ok(Filter::Equal(field, value))
    }
}
