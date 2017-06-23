#![feature(proc_macro, specialization)]
#![allow(dead_code, unused_variables)]

extern crate pyo3;

use pyo3::*;
use std::{isize, iter};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use pyo3::ffi;


macro_rules! py_run {
    ($py:expr, $val:ident, $code:expr) => {{
        let d = PyDict::new($py);
        d.set_item(stringify!($val), &$val).unwrap();
        $py.run($code, None, Some(d)).expect($code);
    }}
}

macro_rules! py_assert {
    ($py:expr, $val:ident, $assertion:expr) => { py_run!($py, $val, concat!("assert ", $assertion)) };
}

macro_rules! py_expect_exception {
    ($py:expr, $val:ident, $code:expr, $err:ident) => {{
        let d = PyDict::new($py);
        d.set_item(stringify!($val), &$val).unwrap();
        let res = $py.run($code, None, Some(d));
        let err = res.unwrap_err();
        if !err.matches($py, $py.get_type::<exc::$err>()) {
            panic!(format!("Expected {} but got {:?}", stringify!($err), err))
        }
    }}
}


#[py::class]
struct EmptyClass { }

#[test]
fn empty_class() {
    let gil = Python::acquire_gil();
    let py = gil.python();
    let typeobj = py.get_type::<EmptyClass>();
    // By default, don't allow creating instances from python.
    assert!(typeobj.call(NoArgs, None).is_err());

    py_assert!(py, typeobj, "typeobj.__name__ == 'EmptyClass'");
}

/// Line1
///Line2
///  Line3
// this is not doc string
#[py::class]
struct ClassWithDocs { }

#[test]
fn class_with_docstr() {
    {
        let gil = Python::acquire_gil();
        let py = gil.python();
        println!("TEST1");
        let typeobj = py.get_type::<ClassWithDocs>();
        println!("TEST2");
        py_run!(py, typeobj, "assert typeobj.__doc__ == 'Line1\\nLine2\\n Line3'");
        println!("TEST3");
    }
    println!("TEST4");
}

#[py::class(name=CustomName)]
struct EmptyClass2 { }

#[test]
fn custom_class_name() {
    let gil = Python::acquire_gil();
    let py = gil.python();
    let typeobj = py.get_type::<EmptyClass2>();
    py_assert!(py, typeobj, "typeobj.__name__ == 'CustomName'");
}

#[py::class]
struct EmptyClassInModule { }

#[test]
fn empty_class_in_module() {
    let gil = Python::acquire_gil();
    let py = gil.python();
    let module = PyModule::new(py, "test_module.nested").unwrap();
    module.add_class::<EmptyClassInModule>().unwrap();

    let ty = module.getattr("EmptyClassInModule").unwrap();
    assert_eq!(ty.getattr("__name__").unwrap().extract::<String>().unwrap(), "EmptyClassInModule");
    assert_eq!(ty.getattr("__module__").unwrap().extract::<String>().unwrap(), "test_module.nested");
}

#[py::class]
struct EmptyClassWithNew {
    token: PyToken
}

#[py::methods]
impl EmptyClassWithNew {
    #[__new__]
    fn __new__(cls: &PyType) -> PyResult<Py<EmptyClassWithNew>> {
        Ok(Py::new(cls.token(), |t| EmptyClassWithNew{token: t})?.into())
    }
}

#[test]
fn empty_class_with_new() {
    let gil = Python::acquire_gil();
    let py = gil.python();
    let typeobj = py.get_type::<EmptyClassWithNew>();
    assert!(typeobj.call(NoArgs, None).unwrap().cast_as::<EmptyClassWithNew>().is_ok());
}

#[py::class]
struct NewWithOneArg {
    _data: i32,
    token: PyToken
}

#[py::methods]
impl NewWithOneArg {
    #[new]
    fn __new__(cls: &PyType, arg: i32) -> PyResult<&mut NewWithOneArg> {
        cls.token().init_mut(|t| NewWithOneArg{_data: arg, token: t})
    }
}

#[test]
fn new_with_one_arg() {
    let gil = Python::acquire_gil();
    let py = gil.python();
    let typeobj = py.get_type::<NewWithOneArg>();
    let wrp = typeobj.call((42,), None).unwrap();
    let obj = wrp.cast_as::<NewWithOneArg>().unwrap();
    assert_eq!(obj._data, 42);
}

#[py::class]
struct NewWithTwoArgs {
    _data1: i32,
    _data2: i32,

    token: PyToken
}

#[py::methods]
impl NewWithTwoArgs {
    #[new]
    fn __new__(cls: &PyType, arg1: i32, arg2: i32) -> PyResult<Py<NewWithTwoArgs>>
    {
        Py::new_ptr(
            cls.token(),
            |t| NewWithTwoArgs{_data1: arg1, _data2: arg2, token: t})
    }
}

#[test]
fn new_with_two_args() {
    let gil = Python::acquire_gil();
    let py = gil.python();
    let typeobj = py.get_type::<NewWithTwoArgs>();
    let wrp = typeobj.call((10, 20), None).unwrap();
    let obj = wrp.cast_as::<NewWithTwoArgs>().unwrap();
    assert_eq!(obj._data1, 10);
    assert_eq!(obj._data2, 20);
}

#[py::class(freelist=10)]
struct ClassWithFreelist{token: PyToken}

#[test]
fn class_with_freelist() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let inst = Py::new_ptr(py, |t| ClassWithFreelist{token: t}).unwrap();
    let inst2 = Py::new_ptr(py, |t| ClassWithFreelist{token: t}).unwrap();
    let ptr = inst.as_ptr();
    drop(inst);

    let inst3 = Py::new_ptr(py, |t| ClassWithFreelist{token: t}).unwrap();
    assert_eq!(ptr, inst3.as_ptr());
}

struct TestDropCall {
    drop_called: Arc<AtomicBool>
}
impl Drop for TestDropCall {
    fn drop(&mut self) {
        self.drop_called.store(true, Ordering::Relaxed);
    }
}

#[py::class]
struct DataIsDropped {
    member1: TestDropCall,
    member2: TestDropCall,
    token: PyToken,
}

#[test]
fn data_is_dropped() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let drop_called1 = Arc::new(AtomicBool::new(false));
    let drop_called2 = Arc::new(AtomicBool::new(false));
    let inst = py.init_ptr(|t| DataIsDropped{
        member1: TestDropCall { drop_called: drop_called1.clone() },
        member2: TestDropCall { drop_called: drop_called2.clone() },
        token: t
    }).unwrap();
    assert!(drop_called1.load(Ordering::Relaxed) == false);
    assert!(drop_called2.load(Ordering::Relaxed) == false);
    drop(inst);
    assert!(drop_called1.load(Ordering::Relaxed) == true);
    assert!(drop_called2.load(Ordering::Relaxed) == true);
}


#[py::class]
struct InstanceMethod {
    member: i32,
    token: PyToken
}

#[py::methods]
impl InstanceMethod {
    /// Test method
    fn method(&self) -> PyResult<i32> {
        Ok(self.member)
    }
}

#[test]
fn instance_method() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let obj = Py::new(py, |t| InstanceMethod{member: 42, token: t}).unwrap();
    assert!(obj.method().unwrap() == 42);
    let d = PyDict::new(py);
    d.set_item("obj", obj).unwrap();
    py.run("assert obj.method() == 42", None, Some(d)).unwrap();
    py.run("assert obj.method.__doc__ == 'Test method'", None, Some(d)).unwrap();
}

#[py::class]
struct InstanceMethodWithArgs {
    member: i32,
    token: PyToken
}

#[py::methods]
impl InstanceMethodWithArgs {
    fn method(&self, multiplier: i32) -> PyResult<i32> {
        Ok(self.member * multiplier)
    }
}

//#[test]
fn instance_method_with_args() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let obj = Py::new(py, |t| InstanceMethodWithArgs{member: 7, token: t}).unwrap();
    assert!(obj.method(6).unwrap() == 42);
    let d = PyDict::new(py);
    d.set_item("obj", obj).unwrap();
    py.run("assert obj.method(3) == 21", None, Some(d)).unwrap();
    py.run("assert obj.method(multiplier=6) == 42", None, Some(d)).unwrap();
}


#[py::class]
struct ClassMethod {token: PyToken}

#[py::methods]
impl ClassMethod {
    #[new]
    fn __new__(cls: &PyType) -> PyResult<Py<ClassMethod>> {
        Py::new_ptr(cls.token(), |t| ClassMethod{token: t})
    }

    #[classmethod]
    fn method(cls: &PyType) -> PyResult<String> {
        Ok(format!("{}.method()!", cls.name()))
    }
}

#[test]
fn class_method() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let d = PyDict::new(py);
    d.set_item("C", py.get_type::<ClassMethod>()).unwrap();
    py.run("assert C.method() == 'ClassMethod.method()!'", None, Some(d)).unwrap();
    py.run("assert C().method() == 'ClassMethod.method()!'", None, Some(d)).unwrap();
}


#[py::class]
struct ClassMethodWithArgs{token: PyToken}

#[py::methods]
impl ClassMethodWithArgs {
    #[classmethod]
    fn method(cls: &PyType, input: &PyString) -> PyResult<String> {
        Ok(format!("{}.method({})", cls.name(), input))
    }
}

#[test]
fn class_method_with_args() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let d = PyDict::new(py);
    d.set_item("C", py.get_type::<ClassMethodWithArgs>()).unwrap();
    py.run("assert C.method('abc') == 'ClassMethodWithArgs.method(abc)'", None, Some(d)).unwrap();
}

#[py::class]
struct StaticMethod {
    token: PyToken
}

#[py::methods]
impl StaticMethod {
    #[new]
    fn __new__(cls: &PyType) -> PyResult<&StaticMethod> {
        Ok(cls.token().init_mut(|t| StaticMethod{token: t})?.into())
    }

    #[staticmethod]
    fn method(py: Python) -> PyResult<&'static str> {
        Ok("StaticMethod.method()!")
    }
}

#[test]
fn static_method() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    assert_eq!(StaticMethod::method(py).unwrap(), "StaticMethod.method()!");
    let d = PyDict::new(py);
    d.set_item("C", py.get_type::<StaticMethod>()).unwrap();
    py.run("assert C.method() == 'StaticMethod.method()!'", None, Some(d)).unwrap();
    py.run("assert C().method() == 'StaticMethod.method()!'", None, Some(d)).unwrap();
}

#[py::class]
struct StaticMethodWithArgs{token: PyToken}

#[py::methods]
impl StaticMethodWithArgs {

    #[staticmethod]
    fn method(py: Python, input: i32) -> PyResult<String> {
        Ok(format!("0x{:x}", input))
    }
}

#[test]
fn static_method_with_args() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    assert_eq!(StaticMethodWithArgs::method(py, 1234).unwrap(), "0x4d2");

    let d = PyDict::new(py);
    d.set_item("C", py.get_type::<StaticMethodWithArgs>()).unwrap();
    py.run("assert C.method(1337) == '0x539'", None, Some(d)).unwrap();
}

#[py::class]
struct GCIntegration {
    self_ref: RefCell<PyObject>,
    dropped: TestDropCall,
    token: PyToken,
}

#[py::proto]
impl PyGCProtocol for GCIntegration {
    fn __traverse__(&self, visit: PyVisit) -> Result<(), PyTraverseError> {
        visit.call(&*self.self_ref.borrow())
    }

    fn __clear__(&mut self) {
        *self.self_ref.borrow_mut() = self.token().None();
    }
}

#[test]
fn gc_integration() {
    let drop_called = Arc::new(AtomicBool::new(false));

    {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let inst = Py::new(py, |t| GCIntegration{
            self_ref: RefCell::new(py.None().into()),
            dropped: TestDropCall { drop_called: drop_called.clone() },
            token: t}).unwrap();

        *inst.self_ref.borrow_mut() = inst.into();
        drop(inst);
    }

    let gil = Python::acquire_gil();
    let py = gil.python();
    py.run("import gc; gc.collect()", None, None).unwrap();
    assert!(drop_called.load(Ordering::Relaxed));
}

#[py::class]
pub struct Len {
    l: usize,
    token: PyToken,
}

#[py::proto]
impl PyMappingProtocol for Len {
    fn __len__(&self) -> PyResult<usize> {
        Ok(self.l)
    }
}

#[test]
fn len() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let inst = Py::new(py, |t| Len{l: 10, token: t}).unwrap();
    py_assert!(py, inst, "len(inst) == 10");
    unsafe {
        assert_eq!(ffi::PyObject_Size(inst.as_ptr()), 10);
        assert_eq!(ffi::PyMapping_Size(inst.as_ptr()), 10);
    }

    let inst = Py::new(py, |t| Len{l: (isize::MAX as usize) + 1, token: t}).unwrap();
    py_expect_exception!(py, inst, "len(inst)", OverflowError);
}

#[py::class]
struct Iterator{
    iter: Box<iter::Iterator<Item=i32> + Send>,
    token: PyToken,
}

#[py::proto]
impl PyIterProtocol for Iterator {
    fn __iter__(&mut self) -> PyResult<Py<Iterator>> {
        Ok(self.into())
    }

    fn __next__(&mut self) -> PyResult<Option<i32>> {
        Ok(self.iter.next())
    }
}

#[test]
fn iterator() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let inst = Py::new(py, |t| Iterator{iter: Box::new(5..8), token: t}).unwrap();
    py_assert!(py, inst, "iter(inst) is inst");
    py_assert!(py, inst, "list(inst) == [5, 6, 7]");
}

#[py::class]
struct StringMethods {token: PyToken}

#[py::proto]
impl<'p> PyObjectProtocol<'p> for StringMethods {
    fn __str__(&self) -> PyResult<&'static str> {
        Ok("str")
    }

    fn __repr__(&self) -> PyResult<&'static str> {
        Ok("repr")
    }

    fn __format__(&self, format_spec: String) -> PyResult<String> {
        Ok(format!("format({})", format_spec))
    }

    fn __unicode__(&self) -> PyResult<PyObject> {
        Ok(PyString::new(self.token(), "unicode").into())
    }

    fn __bytes__(&self) -> PyResult<PyObject> {
        Ok(PyBytes::new(self.token(), b"bytes").into())
    }
}

#[cfg(Py_3)]
#[test]
fn string_methods() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let obj = Py::new(py, |t| StringMethods{token: t}).unwrap();
    py_assert!(py, obj, "str(obj) == 'str'");
    py_assert!(py, obj, "repr(obj) == 'repr'");
    py_assert!(py, obj, "'{0:x}'.format(obj) == 'format(x)'");
    py_assert!(py, obj, "bytes(obj) == b'bytes'");
}

#[cfg(not(Py_3))]
#[test]
fn string_methods() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let obj = Py::new(py, |t| StringMethods{token: t}).unwrap();
    py_assert!(py, obj, "str(obj) == 'str'");
    py_assert!(py, obj, "repr(obj) == 'repr'");
    py_assert!(py, obj, "unicode(obj) == 'unicode'");
    py_assert!(py, obj, "'{0:x}'.format(obj) == 'format(x)'");
}


#[py::class]
struct Comparisons {
    val: i32,
    token: PyToken,
}

#[py::proto]
impl PyObjectProtocol for Comparisons {
    fn __hash__(&self) -> PyResult<usize> {
        Ok(self.val as usize)
    }
    fn __bool__(&self) -> PyResult<bool> {
        Ok(self.val != 0)
    }
}


#[test]
fn comparisons() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let zero = Py::new(py, |t| Comparisons{val: 0, token: t}).unwrap();
    let one = Py::new(py, |t| Comparisons{val: 1, token: t}).unwrap();
    let ten = Py::new(py, |t| Comparisons{val: 10, token: t}).unwrap();
    let minus_one = Py::new(py, |t| Comparisons{val: -1, token: t}).unwrap();
    py_assert!(py, one, "hash(one) == 1");
    py_assert!(py, ten, "hash(ten) == 10");
    py_assert!(py, minus_one, "hash(minus_one) == -2");

    py_assert!(py, one, "bool(one) is True");
    py_assert!(py, zero, "not zero");
}


#[py::class]
struct Sequence {
    token: PyToken
}

#[py::proto]
impl PySequenceProtocol for Sequence {
    fn __len__(&self) -> PyResult<usize> {
        Ok(5)
    }

    fn __getitem__(&self, key: isize) -> PyResult<isize> {
        if key == 5 {
            return Err(PyErr::new::<exc::IndexError, NoArgs>(self.token(), NoArgs));
        }
        Ok(key)
    }
}

#[test]
fn sequence() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| Sequence{token: t}).unwrap();
    py_assert!(py, c, "list(c) == [0, 1, 2, 3, 4]");
    py_expect_exception!(py, c, "c['abc']", TypeError);
}


#[py::class]
struct Callable {token: PyToken}

#[py::methods]
impl Callable {

    #[__call__]
    fn __call__(&self, arg: i32) -> PyResult<i32> {
        Ok(arg * 6)
    }
}

#[test]
fn callable() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| Callable{token: t}).unwrap();
    py_assert!(py, c, "callable(c)");
    py_assert!(py, c, "c(7) == 42");

    let nc = py.init(|t| Comparisons{val: 0, token: t}).unwrap();
    py_assert!(py, nc, "not callable(nc)");
}

#[py::class]
struct SetItem {
    key: i32,
    val: i32,
    token: PyToken,
}

#[py::proto]
impl PyMappingProtocol<'a> for SetItem {
    fn __setitem__(&mut self, key: i32, val: i32) -> PyResult<()> {
        self.key = key;
        self.val = val;
        Ok(())
    }
}

#[test]
fn setitem() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| SetItem{key: 0, val: 0, token: t}).unwrap();
    py_run!(py, c, "c[1] = 2");
    assert_eq!(c.key, 1);
    assert_eq!(c.val, 2);
    py_expect_exception!(py, c, "del c[1]", NotImplementedError);
}

#[py::class]
struct DelItem {
    key: i32,
    token: PyToken,
}

#[py::proto]
impl PyMappingProtocol<'a> for DelItem {
    fn __delitem__(&mut self, key: i32) -> PyResult<()> {
        self.key = key;
        Ok(())
    }
}

#[test]
fn delitem() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| DelItem{key:0, token:t}).unwrap();
    py_run!(py, c, "del c[1]");
    assert_eq!(c.key, 1);
    py_expect_exception!(py, c, "c[1] = 2", NotImplementedError);
}

#[py::class]
struct SetDelItem {
    val: Option<i32>,
    token: PyToken,
}

#[py::proto]
impl PyMappingProtocol for SetDelItem {
    fn __setitem__(&mut self, key: i32, val: i32) -> PyResult<()> {
        self.val = Some(val);
        Ok(())
    }

    fn __delitem__(&mut self, key: i32) -> PyResult<()> {
        self.val = None;
        Ok(())
    }
}

#[test]
fn setdelitem() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| SetDelItem{val: None, token: t}).unwrap();
    py_run!(py, c, "c[1] = 2");
    assert_eq!(c.val, Some(2));
    py_run!(py, c, "del c[1]");
    assert_eq!(c.val, None);
}

#[py::class]
struct Reversed {token: PyToken}

#[py::proto]
impl PyMappingProtocol for Reversed{
    fn __reversed__(&self) -> PyResult<&'static str> {
        Ok("I am reversed")
    }
}

#[test]
fn reversed() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| Reversed{token: t}).unwrap();
    py_run!(py, c, "assert reversed(c) == 'I am reversed'");
}

#[py::class]
struct Contains {token: PyToken}

#[py::proto]
impl PySequenceProtocol for Contains {
    fn __contains__(&self, item: i32) -> PyResult<bool> {
        Ok(item >= 0)
    }
}

#[test]
fn contains() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| Contains{token: t}).unwrap();
    py_run!(py, c, "assert 1 in c");
    py_run!(py, c, "assert -1 not in c");
    py_expect_exception!(py, c, "assert 'wrong type' not in c", TypeError);
}



#[py::class]
struct UnaryArithmetic {token: PyToken}

#[py::proto]
impl PyNumberProtocol for UnaryArithmetic {

    fn __neg__(&self) -> PyResult<&'static str> {
        Ok("neg")
    }

    fn __pos__(&self) -> PyResult<&'static str> {
        Ok("pos")
    }

    fn __abs__(&self) -> PyResult<&'static str> {
        Ok("abs")
    }

    fn __invert__(&self) -> PyResult<&'static str> {
        Ok("invert")
    }
}

#[test]
fn unary_arithmetic() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| UnaryArithmetic{token: t}).unwrap();
    py_run!(py, c, "assert -c == 'neg'");
    py_run!(py, c, "assert +c == 'pos'");
    py_run!(py, c, "assert abs(c) == 'abs'");
    py_run!(py, c, "assert ~c == 'invert'");
}


#[py::class]
struct BinaryArithmetic {
    token: PyToken
}

#[py::proto]
impl PyObjectProtocol for BinaryArithmetic {
    fn __repr__(&self) -> PyResult<&'static str> {
        Ok("BA")
    }
}

#[py::proto]
impl PyNumberProtocol for BinaryArithmetic {
    fn __add__(&self, rhs: &PyInstance) -> PyResult<String> {
        Ok(format!("{:?} + {:?}", self, rhs))
    }

    fn __sub__(&self, rhs: &PyInstance) -> PyResult<String> {
        Ok(format!("{:?} - {:?}", self, rhs))
    }

    fn __mul__(&self, rhs: &PyInstance) -> PyResult<String> {
        Ok(format!("{:?} * {:?}", self, rhs))
    }

    fn __lshift__(&self, rhs: &PyInstance) -> PyResult<String> {
        Ok(format!("{:?} << {:?}", self, rhs))
    }

    fn __rshift__(&self, rhs: &PyInstance) -> PyResult<String> {
        Ok(format!("{:?} >> {:?}", self, rhs))
    }

    fn __and__(&self, rhs: &PyInstance) -> PyResult<String> {
        Ok(format!("{:?} & {:?}", self, rhs))
    }

    fn __xor__(&self, rhs: &PyInstance) -> PyResult<String> {
        Ok(format!("{:?} ^ {:?}", self, rhs))
    }

    fn __or__(&self, rhs: &PyInstance) -> PyResult<String> {
        Ok(format!("{:?} | {:?}", self, rhs))
    }
}

#[test]
fn binary_arithmetic() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| BinaryArithmetic{token: t}).unwrap();
    py_run!(py, c, "assert c + c == 'BA + BA'");
    py_run!(py, c, "assert c + 1 == 'BA + 1'");
    py_run!(py, c, "assert 1 + c == '1 + BA'");
    py_run!(py, c, "assert c - 1 == 'BA - 1'");
    py_run!(py, c, "assert 1 - c == '1 - BA'");
    py_run!(py, c, "assert c * 1 == 'BA * 1'");
    py_run!(py, c, "assert 1 * c == '1 * BA'");

    py_run!(py, c, "assert c << 1 == 'BA << 1'");
    py_run!(py, c, "assert 1 << c == '1 << BA'");
    py_run!(py, c, "assert c >> 1 == 'BA >> 1'");
    py_run!(py, c, "assert 1 >> c == '1 >> BA'");
    py_run!(py, c, "assert c & 1 == 'BA & 1'");
    py_run!(py, c, "assert 1 & c == '1 & BA'");
    py_run!(py, c, "assert c ^ 1 == 'BA ^ 1'");
    py_run!(py, c, "assert 1 ^ c == '1 ^ BA'");
    py_run!(py, c, "assert c | 1 == 'BA | 1'");
    py_run!(py, c, "assert 1 | c == '1 | BA'");
}


#[py::class]
struct RichComparisons {
    token: PyToken
}

#[py::proto]
impl PyObjectProtocol for RichComparisons {
    fn __repr__(&self) -> PyResult<&'static str> {
        Ok("RC")
    }

    fn __richcmp__(&self, other: &PyInstance, op: CompareOp) -> PyResult<String> {
        match op {
            CompareOp::Lt => Ok(format!("{} < {:?}", self.__repr__().unwrap(), other)),
            CompareOp::Le => Ok(format!("{} <= {:?}", self.__repr__().unwrap(), other)),
            CompareOp::Eq => Ok(format!("{} == {:?}", self.__repr__().unwrap(), other)),
            CompareOp::Ne => Ok(format!("{} != {:?}", self.__repr__().unwrap(), other)),
            CompareOp::Gt => Ok(format!("{} > {:?}", self.__repr__().unwrap(), other)),
            CompareOp::Ge => Ok(format!("{} >= {:?}", self.__repr__().unwrap(), other))
        }
    }
}

#[py::class]
struct RichComparisons2 {
    py: PyToken
}

#[py::proto]
impl PyObjectProtocol for RichComparisons2 {
    fn __repr__(&self) -> PyResult<&'static str> {
        Ok("RC2")
    }

    fn __richcmp__(&self, other: &'p PyInstance, op: CompareOp) -> PyResult<PyObject> {
        match op {
            CompareOp::Eq => Ok(true.to_object(self.token())),
            CompareOp::Ne => Ok(false.to_object(self.token())),
            _ => Ok(self.token().NotImplemented())
        }
    }
}

#[test]
fn rich_comparisons() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| RichComparisons{token: t}).unwrap();
    py_run!(py, c, "assert (c < c) == 'RC < RC'");
    py_run!(py, c, "assert (c < 1) == 'RC < 1'");
    py_run!(py, c, "assert (1 < c) == 'RC > 1'");
    py_run!(py, c, "assert (c <= c) == 'RC <= RC'");
    py_run!(py, c, "assert (c <= 1) == 'RC <= 1'");
    py_run!(py, c, "assert (1 <= c) == 'RC >= 1'");
    py_run!(py, c, "assert (c == c) == 'RC == RC'");
    py_run!(py, c, "assert (c == 1) == 'RC == 1'");
    py_run!(py, c, "assert (1 == c) == 'RC == 1'");
    py_run!(py, c, "assert (c != c) == 'RC != RC'");
    py_run!(py, c, "assert (c != 1) == 'RC != 1'");
    py_run!(py, c, "assert (1 != c) == 'RC != 1'");
    py_run!(py, c, "assert (c > c) == 'RC > RC'");
    py_run!(py, c, "assert (c > 1) == 'RC > 1'");
    py_run!(py, c, "assert (1 > c) == 'RC < 1'");
    py_run!(py, c, "assert (c >= c) == 'RC >= RC'");
    py_run!(py, c, "assert (c >= 1) == 'RC >= 1'");
    py_run!(py, c, "assert (1 >= c) == 'RC <= 1'");
}

#[test]
#[cfg(Py_3)]
fn rich_comparisons_python_3_type_error() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c2 = py.init(|t| RichComparisons2{py: t}).unwrap();
    py_expect_exception!(py, c2, "c2 < c2", TypeError);
    py_expect_exception!(py, c2, "c2 < 1", TypeError);
    py_expect_exception!(py, c2, "1 < c2", TypeError);
    py_expect_exception!(py, c2, "c2 <= c2", TypeError);
    py_expect_exception!(py, c2, "c2 <= 1", TypeError);
    py_expect_exception!(py, c2, "1 <= c2", TypeError);
    py_run!(py, c2, "assert (c2 == c2) == True");
    py_run!(py, c2, "assert (c2 == 1) == True");
    py_run!(py, c2, "assert (1 == c2) == True");
    py_run!(py, c2, "assert (c2 != c2) == False");
    py_run!(py, c2, "assert (c2 != 1) == False");
    py_run!(py, c2, "assert (1 != c2) == False");
    py_expect_exception!(py, c2, "c2 > c2", TypeError);
    py_expect_exception!(py, c2, "c2 > 1", TypeError);
    py_expect_exception!(py, c2, "1 > c2", TypeError);
    py_expect_exception!(py, c2, "c2 >= c2", TypeError);
    py_expect_exception!(py, c2, "c2 >= 1", TypeError);
    py_expect_exception!(py, c2, "1 >= c2", TypeError);
}

#[py::class]
struct InPlaceOperations {
    value: u32,
    token: PyToken,
}

#[py::proto]
impl PyObjectProtocol for InPlaceOperations {
    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("IPO({:?})", self.value))
    }
}

#[py::proto]
impl PyNumberProtocol for InPlaceOperations {
    fn __iadd__(&mut self, other: u32) -> PyResult<()> {
        self.value = self.value + other;
        Ok(())
    }

    fn __isub__(&mut self, other: u32) -> PyResult<()> {
        self.value = self.value - other;
        Ok(())
    }

    fn __imul__(&mut self, other: u32) -> PyResult<()> {
        self.value = self.value * other;
        Ok(())
    }

    fn __ilshift__(&mut self, other: u32) -> PyResult<()> {
        self.value = self.value << other;
        Ok(())
    }

    fn __irshift__(&mut self, other: u32) -> PyResult<()> {
        self.value = self.value >> other;
        Ok(())
    }

    fn __iand__(&mut self, other: u32) -> PyResult<()> {
        self.value = self.value & other;
        Ok(())
    }

    fn __ixor__(&mut self, other: u32) -> PyResult<()> {
        self.value = self.value ^ other;
        Ok(())
    }

    fn __ior__(&mut self, other: u32) -> PyResult<()> {
        self.value = self.value | other;
        Ok(())
    }
}

#[test]
fn inplace_operations() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let c = py.init(|t| InPlaceOperations{value: 0, token: t}).unwrap();
    py_run!(py, c, "d = c; c += 1; assert repr(c) == repr(d) == 'IPO(1)'");

    let c = py.init(|t| InPlaceOperations{value:10, token: t}).unwrap();
    py_run!(py, c, "d = c; c -= 1; assert repr(c) == repr(d) == 'IPO(9)'");

    let c = py.init(|t| InPlaceOperations{value: 3, token: t}).unwrap();
    py_run!(py, c, "d = c; c *= 3; assert repr(c) == repr(d) == 'IPO(9)'");

    let c = py.init(|t| InPlaceOperations{value: 3, token: t}).unwrap();
    py_run!(py, c, "d = c; c <<= 2; assert repr(c) == repr(d) == 'IPO(12)'");

    let c = py.init(|t| InPlaceOperations{value: 12, token: t}).unwrap();
    py_run!(py, c, "d = c; c >>= 2; assert repr(c) == repr(d) == 'IPO(3)'");

    let c = py.init(|t| InPlaceOperations{value: 12, token: t}).unwrap();
    py_run!(py, c, "d = c; c &= 10; assert repr(c) == repr(d) == 'IPO(8)'");

    let c = py.init(|t| InPlaceOperations{value: 12, token: t}).unwrap();
    py_run!(py, c, "d = c; c |= 3; assert repr(c) == repr(d) == 'IPO(15)'");

    let c = py.init(|t| InPlaceOperations{value: 12, token: t}).unwrap();
    py_run!(py, c, "d = c; c ^= 5; assert repr(c) == repr(d) == 'IPO(9)'");
}

#[py::class]
struct ContextManager {
    exit_called: bool,
    token: PyToken,
}

#[py::proto]
impl<'p> PyContextProtocol<'p> for ContextManager {

    fn __enter__(&mut self) -> PyResult<i32> {
        Ok(42)
    }

    fn __exit__(&mut self,
                ty: Option<&'p PyType>,
                value: Option<&'p PyInstance>,
                traceback: Option<&'p PyInstance>) -> PyResult<bool> {
        self.exit_called = true;
        if ty == Some(self.token().get_type::<exc::ValueError>()) {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[test]
fn context_manager() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let mut c = py.init_mut(|t| ContextManager{exit_called: false, token: t}).unwrap();
    py_run!(py, c, "with c as x:\n  assert x == 42");
    assert!(c.exit_called);

    c.exit_called = false;
    py_run!(py, c, "with c as x:\n  raise ValueError");
    assert!(c.exit_called);

    c.exit_called = false;
    py_expect_exception!(
        py, c, "with c as x:\n  raise NotImplementedError", NotImplementedError);
    assert!(c.exit_called);
}

#[py::class]
struct ClassWithProperties {
    num: i32,
    token: PyToken,
}

#[py::methods]
impl ClassWithProperties {

    fn get_num(&self) -> PyResult<i32> {
        Ok(self.num)
    }

    #[getter(DATA)]
    fn get_data(&self) -> PyResult<i32> {
        Ok(self.num)
    }
    #[setter(DATA)]
    fn set_data(&mut self, value: i32) -> PyResult<()> {
        self.num = value;
        Ok(())
    }
}


#[test]
fn class_with_properties() {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let inst = py.init(|t| ClassWithProperties{num: 10, token: t}).unwrap();

    py_run!(py, inst, "assert inst.get_num() == 10");
    py_run!(py, inst, "assert inst.get_num() == inst.DATA");
    py_run!(py, inst, "inst.DATA = 20");
    py_run!(py, inst, "assert inst.get_num() == 20");
    py_run!(py, inst, "assert inst.get_num() == inst.DATA");
}
