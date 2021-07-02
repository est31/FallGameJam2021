use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use crate::{rc::Rc, ty::Type, upvalue::UpValue};

pub type IterFn = dyn FnMut() -> Option<Value>;

#[derive(Clone)]
pub enum Value {
    Field(String),
    Ty(Type),
    Blob(usize),
    Instance(usize, Rc<RefCell<HashMap<String, Value>>>),
    Tuple(Rc<Vec<Value>>),
    List(Rc<RefCell<Vec<Value>>>),
    Set(Rc<RefCell<HashSet<Value>>>),
    Dict(Rc<RefCell<HashMap<Value, Value>>>),
    Iter(Type, Rc<RefCell<Box<IterFn>>>),
    Union(HashSet<Value>),
    Float(f64),
    Int(i64),
    Bool(bool),
    String(Rc<String>),
    Function(Rc<Vec<Rc<RefCell<UpValue>>>>, Type, usize),
    ExternFunction(usize),
    /// This value should not be present when running, only when type checking.
    /// Most operations are valid but produce funky results.
    Unknown,
    /// Should not be present when running.
    Nil,
}

impl From<&Type> for Value {
    fn from(ty: &Type) -> Self {
        match ty {
            Type::Field(s) => Value::Field(s.clone()),
            Type::Void => Value::Nil,
            Type::Blob(b) => Value::Blob(*b),
            Type::Instance(b) => Value::Instance(*b, Rc::new(RefCell::new(HashMap::new()))),
            Type::Tuple(fields) => Value::Tuple(Rc::new(fields.iter().map(Value::from).collect())),
            Type::Union(v) => Value::Union(v.iter().map(Value::from).collect()),
            Type::List(v) => Value::List(Rc::new(RefCell::new(vec![Value::from(v.as_ref())]))),
            Type::Set(v) => {
                let mut s = HashSet::new();
                s.insert(Value::from(v.as_ref()));
                Value::Set(Rc::new(RefCell::new(s)))
            }
            Type::Dict(k, v) => {
                let mut s = HashMap::new();
                s.insert(Value::from(k.as_ref()), Value::from(v.as_ref()));
                Value::Dict(Rc::new(RefCell::new(s)))
            }
            Type::Iter(v) => {
                Value::Iter(v.as_ref().clone(), Rc::new(RefCell::new(Box::new(|| None))))
            }
            Type::Unknown | Type::Invalid => Value::Unknown,
            Type::Int => Value::Int(1),
            Type::Float => Value::Float(1.0),
            Type::Bool => Value::Bool(true),
            Type::String => Value::String(Rc::new("".to_string())),
            Type::Function(a, r) => {
                Value::Function(Rc::new(Vec::new()), Type::Function(a.clone(), r.clone()), 0)
            }
            Type::ExternFunction(x) => Value::ExternFunction(*x),
            Type::Ty => Value::Ty(Type::Void),
        }
    }
}

impl From<Type> for Value {
    fn from(ty: Type) -> Self {
        Value::from(&ty)
    }
}

impl Debug for Value {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO(ed): This needs some cleaning
        match self {
            Value::Field(s) => write!(fmt, "( .{} )", s),
            Value::Ty(ty) => write!(fmt, "(type {:?})", ty),
            Value::Blob(b) => write!(fmt, "(blob b{})", b),
            Value::Instance(b, v) => write!(fmt, "(inst b{} {:?})", b, v),
            Value::Float(f) => write!(fmt, "(float {})", f),
            Value::Int(i) => write!(fmt, "(int {})", i),
            Value::Bool(b) => write!(fmt, "(bool {})", b),
            Value::String(s) => write!(fmt, "(string \"{}\")", s),
            Value::List(v) => write!(fmt, "(array {:?})", v),
            Value::Set(v) => write!(fmt, "(set {:?})", v),
            Value::Dict(v) => write!(fmt, "(dict {:?})", v),
            Value::Iter(v, _) => write!(fmt, "(iter {:?})", v),
            Value::Function(_, ty, block) => {
                write!(fmt, "(fn #{} {:?})", block, ty)
            }
            Value::ExternFunction(slot) => write!(fmt, "(extern fn {})", slot),
            Value::Unknown => write!(fmt, "(unknown)"),
            Value::Nil => write!(fmt, "(nil)"),
            Value::Tuple(v) => write!(fmt, "({:?})", v),
            Value::Union(v) => write!(fmt, "(U {:?})", v),
        }
    }
}

impl PartialEq<Value> for Value {
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Tuple(a), Value::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| a == b)
            }
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            (Value::Dict(a), Value::Dict(b)) => a == b,
            (Value::Union(a), b) | (b, Value::Union(a)) => a.iter().any(|x| x == b),
            (Value::Nil, Value::Nil) => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Value::Float(a) => {
                // We have to limit the values, because
                // floats are wierd.
                assert!(a.is_finite());
                a.to_bits().hash(state);
            }
            Value::Int(a) => a.hash(state),
            Value::Bool(a) => a.hash(state),
            Value::String(a) => a.hash(state),
            Value::Tuple(a) => a.hash(state),
            Value::Nil => state.write_i8(0),
            _ => {}
        };
    }
}

impl Value {
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }
}

#[derive(Clone)]
pub enum MatchableValue<'t> {
    Empty,
    One(&'t Value),
    Two(&'t Value, &'t Value),
    Three(&'t Value, &'t Value, &'t Value),
    Four(&'t Value, &'t Value, &'t Value, &'t Value),
    Five(&'t Value, &'t Value, &'t Value, &'t Value, &'t Value),
}

pub fn make_matchable<'t>(value: &'t Value) -> MatchableValue<'t> {
    use MatchableValue::*;
    use Value::*;

    match value {
        #[rustfmt::skip]
        Tuple(inner) => {
            match (inner.get(0), inner.get(1), inner.get(2), inner.get(3), inner.get(4)) {
                (Some(a), Some(b), Some(c), Some(d), Some(e), ..) => Five(a, b, c, d, e),
                (Some(a), Some(b), Some(c), Some(d), ..) => Four(a, b, c, d),
                (Some(a), Some(b), Some(c), ..) => Three(a, b, c),
                (Some(a), Some(b), ..) => Two(a, b),
                (Some(a), ..) => One(a),
                _ => Empty,
            }
        },
        x => One(x),
    }
}