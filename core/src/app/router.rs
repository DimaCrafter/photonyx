use std::collections::HashMap;
use crate::{context::http::HttpContext, http::{codes::HttpCode, entity::{HttpConnection, Request, Response, ResponseExit, ResponseRet}}, utils::log::log_info};

type ActionFn = dyn Fn(&mut HttpContext) -> ResponseRet + Sync + Send + 'static;
type MiddlewareFn = dyn Fn(&mut HttpContext) -> MiddlewareResult + Sync + Send + 'static;

#[repr(u8)]
pub enum MiddlewareResult {
    Continue,
    Stop
}

pub struct Route {
    pub matcher: PathMatcher,
    pub call: Box<ActionFn>,
    // Module name can be used for unloading later
    pub origin_module: Option<String>
}

impl Route {
    pub fn new (pattern: String, action: Box<ActionFn>) -> Self {
        return Route {
            matcher: PathMatcher::from_pattern(pattern),
            call: action,
            origin_module: None
        };
    }
}

pub struct Router {
    pub routes: Vec<Route>,
    middlewares: Vec<(WildcardMatcher, Box<MiddlewareFn>)>,
    middleware_wildcards: Vec<Box<MiddlewareFn>>,
    origin_module: Option<String>
}

impl Router {
    pub const fn empty () -> Self {
        Router {
            routes: Vec::new(),
            middlewares: Vec::new(),
            middleware_wildcards: Vec::new(),
            origin_module: None
        }
    }

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

    pub fn use_middleware<Caller: Fn(&mut HttpContext) -> MiddlewareResult + Sync + Send + 'static> (&mut self, pattern: String, middleware: Caller) {
        let reg_msg = format!("registered middleware '{pattern}' to {:p}", &middleware);

        if pattern == "*" {
            self.middleware_wildcards.push(Box::new(middleware));
        } else {
            self.middlewares.push((WildcardMatcher::from_pattern(pattern), Box::new(middleware)));
        }

        if let Some(ref mod_name) = self.origin_module {
            log_info(&format!("{mod_name}: {reg_msg}"));
        } else {
            log_info(&format!("core: {reg_msg}"));
        }
    }

    pub fn match_action (&self, path: &String) -> Option<(&Route, HashMap<String, String>)> {
        for route in &self.routes {
            if let Some(params) = route.matcher.exec(&path) {
                return Some((route, params));
            }
        }

        return None;
    }

    pub fn call_action<Connection: HttpConnection> (&self, connection: &Connection, req: Request) -> Option<Response> {
        let (endpoint, params) = match self.match_action(&req.path) {
            Some(pair) => pair,
            None => return None
        };

        let mut ctx = HttpContext::from(connection, req, params);
        for (pattern, middleware) in &self.middlewares {
            if pattern.exec(&ctx.req.path) {
                match middleware(&mut ctx) {
                    MiddlewareResult::Continue => continue,
                    MiddlewareResult::Stop => break
                }
            }
        }

        if !matches!(ctx.res.code, HttpCode::NotSent) {
            return Some(ctx.finalize());
        }

        for middleware in &self.middleware_wildcards {
            match middleware(&mut ctx) {
                MiddlewareResult::Continue => continue,
                MiddlewareResult::Stop => break
            }
        }

        if !matches!(ctx.res.code, HttpCode::NotSent) {
            return Some(ctx.finalize());
        }

        match (endpoint.call)(&mut ctx) {
            Err(ResponseExit::Replace(response)) => {
                ctx.res = response;
                return Some(ctx.finalize());
            },
            _ => {
                return Some(ctx.finalize());
            }
        }
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

    pub fn exec (&self, path: &str) -> Option<HashMap<String, String>> {
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


pub struct WildcardMatcher {
    parts: Vec<String>
}

impl WildcardMatcher {
    pub fn from_pattern (pattern: String) -> Self {
        return WildcardMatcher { parts: pattern.split('*').map(str::to_owned).collect() };
    }

    pub fn exec (&self, path: &str) -> bool {
        let (first, rest) = self.parts.split_first().expect("split gives at least one part");
        let Some((last, middle)) = rest.split_last() else {
            return path == first;
        };

        let Some(mut tail) = path.strip_prefix(first.as_str()) else {
            return false;
        };

        for part in middle {
            match tail.find(part.as_str()) {
                Some(index) => tail = &tail[index + part.len()..],
                None => return false
            }
        }

        return tail.len() >= last.len() && tail.ends_with(last.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::WildcardMatcher;

    fn matches (pattern: &str, path: &str) -> bool {
        return WildcardMatcher::from_pattern(pattern.to_owned()).exec(path);
    }

    #[test]
    fn wildcard () {
        assert!(matches("/order/confirm", "/order/confirm"));
        assert!(!matches("/order/confirm", "/order/confirm/x"));
        assert!(!matches("/order/confirm", "/order"));

        assert!(matches("/profile/*", "/profile/ping"));
        assert!(matches("/profile/*", "/profile/a/b"));
        assert!(matches("/profile/*", "/profile/"));
        assert!(!matches("/profile/*", "/profile"));
        assert!(!matches("/profile/*", "/profiles/x"));

        assert!(matches("/admin*", "/admin"));
        assert!(matches("/admin*", "/admin-users/ping"));
        assert!(!matches("/admin*", "/adm"));

        assert!(matches("*", "/anything"));
        assert!(matches("/a/*/c", "/a/b/c"));
        assert!(!matches("/a/*/c", "/a/b/d"));
        assert!(matches("*.json", "/x/y.json"));
        assert!(!matches("/a*a", "/a"));
    }
}
