use std::collections::HashMap;
use crate::{context::http::HttpContext, http::entity::ResponseRet, utils::log::log_info};

type ActionCallerType = dyn Fn(&mut HttpContext) -> ResponseRet + Sync + Send + 'static;

pub struct Route {
    pub matcher: PathMatcher,
    pub call: Box<ActionCallerType>,
    // Module name can be used for unloading later
    pub origin_module: Option<String>
}

impl Route {
    pub fn new (pattern: String, action: Box<ActionCallerType>) -> Self {
        return Route {
            matcher: PathMatcher::from_pattern(pattern),
            call: action,
            origin_module: None
        };
    }
}

pub struct Router {
    pub routes: Vec<Route>,
    origin_module: Option<String>
}

impl Router {
    pub const fn empty () -> Self {
        Router {
            routes: Vec::new(),
            origin_module: None
        }
    }

    #[inline]
    pub fn register<Caller: Fn(&mut HttpContext) -> ResponseRet + Sync + Send + 'static> (&mut self, pattern: String, action: Caller) {
        let reg_msg = format!("registered route '{pattern}' to {:p}", &action);

        let mut route = Route::new(pattern, Box::new(action));
        if let Some(ref mod_name) = self.origin_module {
            route.origin_module = Some(mod_name.clone());
            log_info(&format!("{mod_name}: {reg_msg}"));
        } else {
            log_info(&format!("core: {reg_msg}"));
        }

        self.routes.push(route);
    }

    pub fn match_path (&self, path: &String) -> Option<(&Route, HashMap<String, String>)> {
        for route in &self.routes {
            if let Some(params) = route.matcher.exec(&path) {
                return Some((route, params));
            }
        }

        return None;
    }

    pub fn with_module<C: Fn (&mut Self)> (&mut self, name: &str, consume: C) {
        self.origin_module = Some(name.to_owned());
        consume(self);
        self.origin_module = None;
    }
}

enum PathPart {
    String(String),
    Variable(String, char)
}

pub struct PathMatcher(Vec<PathPart>);
impl PathMatcher {
    pub fn from_pattern (pattern: String) -> Self {
        let mut sequence = Vec::new();

        let mut tmp = String::new();
        let mut is_var = false;
        let mut is_var_end = false;

        for ch in pattern.chars() {
            if is_var {
                if ch == '}' {
                    is_var = false;
                    is_var_end = true;
                } else {
                    tmp.push(ch);
                }
            } else if ch == '{' {
                if tmp.len() != 0 {
                    sequence.push(PathPart::String(tmp));
                    tmp = String::new();
                }

                is_var = true;
            } else if is_var_end {
                sequence.push(PathPart::Variable(tmp, ch));
                tmp = String::new();
            } else {
                tmp.push(ch);
            }
        }

        if is_var_end {
            sequence.push(PathPart::Variable(tmp, '\0'));
        } else if tmp.len() != 0 {
            sequence.push(PathPart::String(tmp));
        }

        return PathMatcher(sequence);
    }

    pub fn exec (&self, path: &String) -> Option<HashMap<String, String>> {
        let mut offset = 0usize;
        let mut path_iter = path.chars();
        let mut params: HashMap<String, String> = HashMap::new();

        for part in &self.0 {
            match part {
                PathPart::String(value) => {
                    let mut part_iter = value.chars();
                    loop {
                        if let Some(ch) = part_iter.next() {
                            let next_ch = path_iter.next();
                            if next_ch.is_none() || ch != next_ch.unwrap() {
                                return None;
                            }
                        } else {
                            if let None = path_iter.next() { break; }
                            else { return None; }
                        }
                    }

                    offset += value.len();
                }
                PathPart::Variable(name, stop_char) => {
                    let mut i = 0usize;
                    loop {
                        let next = path_iter.next();
                        if next.is_none() || next.unwrap() == *stop_char { break }
                        i += 1;
                    }

                    if offset >= path.len() { return None; }
                    let value = &path[(offset)..(offset + i)];
                    offset += i + 1;
					params.insert(name.to_string(), value.to_string());
                }
            }
        }

        return Some(params);
    }
}
