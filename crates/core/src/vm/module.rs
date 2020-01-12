use super::address::*;
use super::export::{ExportInstance, ExternalValue};
use super::global::DefinedGlobalInstance;
use super::host::*;
use super::memory::DefinedMemoryInstance;
use super::table::DefinedTableInstance;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct ModuleIndex(pub u32);

pub enum ModuleInstance {
    Defined(DefinedModuleInstance),
    Host(HostModuleInstance),
}

impl ModuleInstance {
    pub fn defined(&self) -> Option<&DefinedModuleInstance> {
        match self {
            ModuleInstance::Defined(defined) => Some(defined),
            _ => None,
        }
    }
}

pub struct DefinedModuleInstance {
    types: Vec<parity_wasm::elements::Type>,
    pub exports: Vec<ExportInstance>,
    start_func: Option<FuncAddr>,
}

pub enum DefinedModuleError {
    TypeMismatch(&'static str, String),
}

impl std::fmt::Display for DefinedModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TypeMismatch(expected, actual) => write!(
                f,
                "incompatible import type, expected {} but actual {}",
                expected, actual
            ),
        }
    }
}

type DefinedModuleResult<T> = std::result::Result<T, DefinedModuleError>;

impl DefinedModuleInstance {
    pub fn new_from_parity_module(
        module: parity_wasm::elements::Module,
        module_index: ModuleIndex,
        types: Vec<parity_wasm::elements::Type>,
    ) -> Self {
        Self {
            types,
            exports: module
                .export_section()
                .map(|sec| sec.entries().iter())
                .map(|entries| {
                    entries.map(|e| ExportInstance::new_from_parity_entry(e.clone(), module_index))
                })
                .map(|s| s.collect())
                .unwrap_or_default(),
            start_func: module
                .start_section()
                .map(|func_index| FuncAddr(module_index, func_index as usize)),
        }
    }

    pub fn exported_by_name(&self, name: String) -> Option<&ExportInstance> {
        self.exports.iter().filter(|e| *e.name() == name).next()
    }

    pub fn exported_global(&self, name: String) -> DefinedModuleResult<Option<GlobalAddr>> {
        let export = self.exported_by_name(name);
        match export {
            Some(e) => match e.value() {
                ExternalValue::Global(addr) => Ok(Some(addr.clone())),
                _ => Err(DefinedModuleError::TypeMismatch(
                    "global",
                    e.value().ty().to_string(),
                )),
            },
            None => Ok(None),
        }
    }

    pub fn exported_func(&self, name: String) -> DefinedModuleResult<Option<FuncAddr>> {
        let export = self.exported_by_name(name);
        match export {
            Some(e) => match e.value() {
                ExternalValue::Func(addr) => Ok(Some(addr.clone())),
                _ => Err(DefinedModuleError::TypeMismatch(
                    "function",
                    e.value().ty().to_string(),
                )),
            },
            None => Ok(None),
        }
    }

    pub fn exported_table(&self, name: String) -> DefinedModuleResult<Option<TableAddr>> {
        let export = self.exported_by_name(name);
        match export {
            Some(e) => match e.value() {
                ExternalValue::Table(addr) => Ok(Some(addr.clone())),
                _ => Err(DefinedModuleError::TypeMismatch(
                    "table",
                    e.value().ty().to_string(),
                )),
            },
            None => Ok(None),
        }
    }

    pub fn exported_memory(&self, name: String) -> DefinedModuleResult<Option<MemoryAddr>> {
        let export = self.exported_by_name(name);
        match export {
            Some(e) => match e.value() {
                ExternalValue::Memory(addr) => Ok(Some(addr.clone())),
                _ => Err(DefinedModuleError::TypeMismatch(
                    "memory",
                    e.value().ty().to_string(),
                )),
            },
            None => Ok(None),
        }
    }

    pub fn start_func_addr(&self) -> &Option<FuncAddr> {
        &self.start_func
    }

    pub fn get_type(&self, index: usize) -> &parity_wasm::elements::Type {
        &self.types[index]
    }
}

pub struct HostModuleInstance {
    funcs: HashMap<String, ExecutableFuncAddr>,
    globals: HashMap<String, Rc<RefCell<DefinedGlobalInstance>>>,
    tables: HashMap<String, Rc<RefCell<DefinedTableInstance>>>,
    mems: HashMap<String, Rc<RefCell<DefinedMemoryInstance>>>,
}

pub enum HostModuleError {
    TypeMismatch(&'static str, String),
}

impl std::fmt::Display for HostModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TypeMismatch(expected, actual) => write!(
                f,
                "incompatible import type, expected {} but actual {}",
                expected, actual
            ),
        }
    }
}

type HostModuleResult<T> = std::result::Result<T, HostModuleError>;

impl HostModuleInstance {
    pub fn new(
        funcs: HashMap<String, ExecutableFuncAddr>,
        globals: HashMap<String, Rc<RefCell<DefinedGlobalInstance>>>,
        tables: HashMap<String, Rc<RefCell<DefinedTableInstance>>>,
        mems: HashMap<String, Rc<RefCell<DefinedMemoryInstance>>>,
    ) -> Self {
        Self {
            funcs,
            globals,
            tables,
            mems,
        }
    }

    pub fn global_by_name(
        &self,
        name: String,
    ) -> HostModuleResult<Option<&Rc<RefCell<DefinedGlobalInstance>>>> {
        Ok(self.globals.get(&name))
    }
    pub fn func_by_name(&self, name: String) -> Option<&ExecutableFuncAddr> {
        self.funcs.get(&name)
    }

    #[deprecated]
    pub fn _func_by_name(&self, name: String) -> HostModuleResult<Option<&HostFuncBody>> {
        panic!()
    }

    pub fn table_by_name(
        &self,
        name: String,
    ) -> HostModuleResult<Option<&Rc<RefCell<DefinedTableInstance>>>> {
        Ok(self.tables.get(&name))
    }

    pub fn memory_by_name(
        &self,
        name: String,
    ) -> HostModuleResult<Option<&Rc<RefCell<DefinedMemoryInstance>>>> {
        Ok(self.mems.get(&name))
    }
}
