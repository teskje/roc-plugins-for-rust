use std::ffi::c_void;
use std::fs::{self, File};
use std::io::Write;
use std::panic;
use std::path::Path;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::LazyLock;

use libloading::{Library, Symbol};
use regex::Regex;
use roc_std::RocStr;

#[derive(Debug)]
struct Meta {
    name: String,
    arg_types: Vec<DType>,
    return_type: DType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DType {
    Str,
    U64,
}

impl DType {
    fn as_str(&self) -> &str {
        match self {
            Self::Str => "Str",
            Self::U64 => "U64",
        }
    }
}

impl FromStr for DType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let dtype = match s {
            "Str" => Self::Str,
            "U64" => Self::U64,
            _ => return Err(s.into()),
        };
        Ok(dtype)
    }
}

#[derive(Debug)]
enum Value {
    Str(RocStr),
    U64(u64),
}

impl Value {
    fn as_void_ptr(&self) -> *const c_void {
        match self {
            Value::Str(s) => s as *const _ as *const _,
            Value::U64(n) => *n as *const _,
        }
    }
}

#[derive(Debug)]
pub struct Plugin {
    meta: Meta,
    dylib: Library,
}

impl Plugin {
    pub fn name(&self) -> &str {
        &self.meta.name
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Self {
        let code = fs::read_to_string(path).unwrap();

        let (header, code) = code.split_once('\n').unwrap();
        let meta = parse_header(header);

        let dylib = compile(&meta, code);

        Self { meta, dylib }
    }

    pub fn invoke(&self) {
        let result = catch_unwind_silent(|| match &self.meta.arg_types[..] {
            [] => self.invoke0(),
            [t1] => self.invoke1(*t1),
            [t1, t2] => self.invoke2(*t1, *t2),
            _ => unimplemented!("more than 2 arguments"),
        });

        if let Err(error) = result {
            let msg = error.downcast::<String>().unwrap();
            eprintln!("plugin panicked: {}", *msg);
        }
    }

    unsafe fn get_entrypoint<F>(&self) -> Symbol<F> {
        self.dylib.get(b"roc__entry_1_exposed_generic").unwrap()
    }

    fn invoke0(&self) {
        match self.meta.return_type {
            DType::Str => {
                let mut result = RocStr::default();
                unsafe {
                    let entry = self.get_entrypoint::<unsafe extern "C" fn(*mut RocStr)>();
                    entry(&mut result);
                }
                println!(">>> {result}");
            }
            DType::U64 => {
                let result = unsafe {
                    let entry = self.get_entrypoint::<unsafe extern "C" fn() -> u64>();
                    entry()
                };
                println!(">>> {result}");
            }
        }
    }

    fn invoke1(&self, t1: DType) {
        let a1 = generate_value(t1);

        match self.meta.return_type {
            DType::Str => {
                let mut result = RocStr::default();
                unsafe {
                    let entry =
                        self.get_entrypoint::<unsafe extern "C" fn(*mut RocStr, *const c_void)>();
                    entry(&mut result, a1.as_void_ptr());
                }
                println!(">>> {result}");
            }
            DType::U64 => {
                let result = unsafe {
                    let entry = self.get_entrypoint::<unsafe extern "C" fn(*const c_void) -> u64>();
                    entry(a1.as_void_ptr())
                };
                println!(">>> {result}");
            }
        }
    }

    fn invoke2(&self, t1: DType, t2: DType) {
        let a1 = generate_value(t1);
        let a2 = generate_value(t2);

        match self.meta.return_type {
            DType::Str => {
                let mut result = RocStr::default();
                unsafe {
                    let entry =
                        self.get_entrypoint::<unsafe extern "C" fn(*mut RocStr, *const c_void, *const c_void)>();
                    entry(&mut result, a1.as_void_ptr(), a2.as_void_ptr());
                }
                println!(">>> {result}");
            }
            DType::U64 => {
                let mut result = 0;
                unsafe {
                    let entry = self.get_entrypoint::<unsafe extern "C" fn(*mut u64, *const c_void, *const c_void)>();
                    entry(&mut result, a1.as_void_ptr(), a2.as_void_ptr())
                };
                println!(">>> {result}");
            }
        }
    }
}

fn parse_header(header: &str) -> Meta {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^#\[plugin\] (?P<name>\w+) : ((?P<args>[\w, ]+) -> )?(?P<ret>\w+)$").unwrap()
    });

    let caps = RE.captures(header).unwrap();
    let name = &caps["name"];
    let args = caps.name("args").map_or("", |m| m.as_str());
    let ret = &caps["ret"];

    let arg_types = args
        .split_terminator(", ")
        .map(|x| x.parse().unwrap())
        .collect();
    let return_type = ret.parse().unwrap();

    Meta {
        name: name.into(),
        arg_types,
        return_type,
    }
}

fn compile(meta: &Meta, code: &str) -> Library {
    let tmpdir = tempfile::tempdir().unwrap();
    let platform_file_path = tmpdir.path().join("platform.roc");
    let app_file_path = tmpdir.path().join("plugin.roc");
    let dylib_file_path = tmpdir.path().join("plugin.dylib");

    let platform_file = File::create(&platform_file_path).unwrap();
    let platform_code = gen_platform_code(&meta);
    write!(&platform_file, "{platform_code}").unwrap();

    let app_file = File::create(&app_file_path).unwrap();
    let app_header = format!(
        r#"app [{name}] {{ pf: platform "{path}" }}"#,
        name = meta.name,
        path = platform_file_path.to_str().unwrap(),
    );
    write!(&app_file, "{app_header}\n").unwrap();
    write!(&app_file, "{code}").unwrap();

    let status = Command::new("roc")
        .args(["build", "--lib"])
        .args(["--output", dylib_file_path.to_str().unwrap()])
        .arg(app_file_path)
        .stdout(Stdio::null())
        .status()
        .unwrap();

    if !status.success() {
        panic!("roc compile failed: {status}");
    }

    unsafe { Library::new(&dylib_file_path).unwrap() }
}

fn gen_platform_code(meta: &Meta) -> String {
    if meta.arg_types.is_empty() {
        format!(
            r#"
platform "plugin"
    requires {{}} {{ {name} : {return_type} }}
    exposes []
    packages {{}}
    imports []
    provides [entry]

entry = {name}"#,
            name = meta.name,
            return_type = meta.return_type.as_str(),
        )
    } else {
        let arg_types: String = meta
            .arg_types
            .iter()
            .map(|t| t.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let arg_vars = ('a'..)
            .map(|x| x.to_string())
            .take(meta.arg_types.len())
            .collect::<Vec<_>>();

        format!(
            r#"
platform "plugin"
    requires {{}} {{ {name} : {arg_types} -> {return_type} }}
    exposes []
    packages {{}}
    imports []
    provides [entry]

entry = \{args1} -> {name} {args2}"#,
            name = meta.name,
            return_type = meta.return_type.as_str(),
            args1 = arg_vars.join(", "),
            args2 = arg_vars.join(" "),
        )
    }
}

fn generate_value(t: DType) -> Value {
    match t {
        DType::Str => Value::Str("foo".into()),
        DType::U64 => Value::U64(42),
    }
}

fn catch_unwind_silent<F: FnOnce() -> R + panic::UnwindSafe, R>(f: F) -> std::thread::Result<R> {
    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(f);
    panic::set_hook(prev_hook);
    result
}
