use super::value::Value;

use std::cell::RefCell;
use std::rc::Rc;

use super::address::MemoryAddr;
use super::executor::Trap;
use super::global::DefinedGlobalInstance;
use super::memory::DefinedMemoryInstance;
use super::module::ModuleIndex;
use super::store::Store;
use super::table::DefinedTableInstance;
use parity_wasm::elements::FunctionType;

type Ref<T> = Rc<RefCell<T>>;

pub struct HostContext<'a> {
    pub mem: &'a mut [u8],
}

pub enum HostValue {
    Func(HostFuncBody),
    Global(Rc<RefCell<DefinedGlobalInstance>>),
    Mem(Ref<DefinedMemoryInstance>),
    Table(Ref<DefinedTableInstance>),
}

impl HostValue {
    pub fn ty(&self) -> &str {
        match self {
            Self::Func(_) => "function",
            Self::Global(_) => "global",
            Self::Mem(_) => "memory",
            Self::Table(_) => "table",
        }
    }
}

pub struct HostFuncBody {
    ty: FunctionType,
    code: Box<dyn Fn(&[Value], &mut [Value], &mut HostContext, &mut Store) -> Result<(), Trap>>,
}

impl HostFuncBody {
    pub fn new<F>(ty: FunctionType, code: F) -> Self
    where
        F: Fn(&[Value], &mut [Value], &mut HostContext, &mut Store) -> Result<(), Trap>,
        F: 'static,
    {
        Self {
            ty,
            code: Box::new(code),
        }
    }

    pub fn call(
        &self,
        param: &[Value],
        results: &mut [Value],
        store: &mut Store,
        module_index: ModuleIndex,
    ) -> Result<(), Trap> {
        if store.memory_count(module_index) > 0 {
            let mem_addr = MemoryAddr(module_index, 0);
            let mem = store.memory(mem_addr);
            let mem = mem.borrow().resolve_memory_instance(store).clone();
            let mem = &mut mem.borrow_mut();
            let raw_mem = mem.raw_data_mut();
            let mut ctx = HostContext { mem: raw_mem };
            (self.code)(param, results, &mut ctx, store)
        } else {
            let mut ctx = HostContext { mem: &mut vec![] };
            (self.code)(param, results, &mut ctx, store)
        }
    }

    pub fn ty(&self) -> &FunctionType {
        &self.ty
    }
}
