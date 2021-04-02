use futures::SinkExt;
use std::{collections::HashMap, sync::mpsc};
use std::{
    sync::{Arc, Mutex},
    thread,
};
use tokio_tungstenite::tungstenite::Message;
use wasmparser::FuncType;

use crate::rpc::{self};
use crate::serialization;
use wasminspect_debugger::{CommandContext, CommandResult, MainDebugger, Process};
use wasminspect_vm::{HostFuncBody, HostValue, Trap, WasmValue};

static VERSION: &str = "0.1.0";

pub fn handle_request<S: futures::Sink<Message> + Unpin + Send + 'static>(
    req: rpc::Request,
    process: &mut Process<MainDebugger>,
    context: &CommandContext,
    tx: Arc<Mutex<S>>,
    rx: Arc<mpsc::Receiver<Option<Message>>>,
) -> rpc::Response
where
    S::Error: std::error::Error,
{
    match _handle_request(req, process, context, tx, rx) {
        Ok(res) => res,
        Err(err) => rpc::TextResponse::Error {
            message: err.to_string(),
        }
        .into(),
    }
}

fn from_js_number(value: rpc::JSNumber, ty: &wasmparser::Type) -> WasmValue {
    match ty {
        wasmparser::Type::I32 => wasminspect_vm::WasmValue::I32(value as i32),
        wasmparser::Type::I64 => wasminspect_vm::WasmValue::I64(value as i64),
        wasmparser::Type::F32 => {
            wasminspect_vm::WasmValue::F32(u32::from_le_bytes((value as f32).to_le_bytes()))
        }
        wasmparser::Type::F64 => {
            wasminspect_vm::WasmValue::F64(u64::from_le_bytes((value as f64).to_le_bytes()))
        }
        _ => unreachable!(),
    }
}

#[allow(dead_code)]
fn to_vm_wasm_value(value: &rpc::WasmValue) -> WasmValue {
    match value {
        rpc::WasmValue::F32 { value } => WasmValue::F32(*value),
        rpc::WasmValue::F64 { value } => WasmValue::F64(*value),
        rpc::WasmValue::I32 { value } => WasmValue::I32(*value),
        rpc::WasmValue::I64 { value } => WasmValue::I64(*value),
    }
}

fn from_vm_wasm_value(value: &WasmValue) -> rpc::WasmValue {
    match value {
        WasmValue::F32(v) => rpc::WasmValue::F32 { value: *v },
        WasmValue::F64(v) => rpc::WasmValue::F64 { value: *v },
        WasmValue::I32(v) => rpc::WasmValue::I32 { value: *v },
        WasmValue::I64(v) => rpc::WasmValue::I64 { value: *v },
    }
}

#[derive(Debug)]
struct RemoteCallError(String);
impl std::fmt::Display for RemoteCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for RemoteCallError {}

fn remote_call_fn<S: futures::Sink<Message> + Unpin + Send + 'static>(
    field_name: String,
    module_name: String,
    ty: FuncType,
    tx: Arc<Mutex<S>>,
    rx: Arc<mpsc::Receiver<Option<Message>>>,
) -> HostFuncBody
where
    S::Error: std::error::Error,
{
    let tx = tx.clone();
    let rx = rx.clone();

    HostFuncBody::new(ty.clone(), move |args, results, _, _| {
        let tx = tx.clone();
        let field_name = field_name.clone();
        let module_name = module_name.clone();
        let args = args.iter().map(from_vm_wasm_value).collect();
        let call_handle = thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let call = rpc::TextResponse::CallHost {
                    module: module_name,
                    field: field_name,
                    args: args,
                };
                tx.lock()
                    .unwrap()
                    .send(serialization::serialize_response(call.into()))
                    .await
                    .unwrap();
            });
        });

        call_handle.join().map_err(|e| {
            let e = RemoteCallError(format!("{:?}", e));
            Trap::HostFunctionError(Box::new(e))
        })?;

        let message = rx
            .recv()
            .map_err(|e| Trap::HostFunctionError(Box::new(e)))?
            .ok_or(RemoteCallError("unexpected end of message".to_owned()))
            .map_err(|e| Trap::HostFunctionError(Box::new(e)))?;
        let res = match serialization::deserialize_request(&message)
            .map_err(|e| Trap::HostFunctionError(Box::new(e)))?
        {
            rpc::Request::Text(rpc::TextRequest::CallResult { values }) => values,
            _ => unreachable!(),
        };
        *results = res
            .iter()
            .zip(ty.params.iter())
            .map(|(arg, ty)| from_js_number(*arg, ty))
            .collect::<Vec<WasmValue>>();
        Ok(())
    })
}

fn remote_import_module<S: futures::Sink<Message> + Unpin + Send + 'static>(
    bytes: &[u8],
    tx: Arc<Mutex<S>>,
    rx: Arc<mpsc::Receiver<Option<Message>>>,
) -> anyhow::Result<HashMap<String, HashMap<String, HostValue>>>
where
    S::Error: std::error::Error,
{
    let parser = wasmparser::Parser::new(0);
    let mut types = HashMap::new();
    let mut module_imports = HashMap::new();
    let mut modules: HashMap<String, HashMap<String, HostValue>> = HashMap::new();

    for payload in parser.parse_all(bytes) {
        match payload? {
            wasmparser::Payload::TypeSection(mut iter) => {
                for idx in 0..iter.get_count() {
                    let ty = iter.read()?;
                    types.insert(idx, ty);
                }
            }
            wasmparser::Payload::ImportSection(iter) => {
                for import in iter {
                    let import = import?;
                    module_imports.insert((import.module, import.field), import);

                    let ty_idx = match import.ty {
                        wasmparser::ImportSectionEntryType::Function(ty_idx) => ty_idx,
                        _ => continue,
                    };
                    let ty = match types.get(&ty_idx) {
                        Some(wasmparser::TypeDef::Func(ty)) => ty,
                        _ => continue,
                    };
                    let field_name = match import.field {
                        Some(field_name) => field_name,
                        None => continue,
                    };

                    let func = remote_call_fn(
                        field_name.to_string(),
                        import.module.to_string(),
                        ty.clone(),
                        tx.clone(),
                        rx.clone(),
                    );
                    modules
                        .entry(import.module.to_string())
                        .or_default()
                        .insert(field_name.to_string(), HostValue::Func(func));
                }
            }
            _ => continue,
        }
    }
    Ok(modules)
}

fn _handle_request<S: futures::Sink<Message> + Unpin + Send + 'static>(
    req: rpc::Request,
    process: &mut Process<MainDebugger>,
    context: &CommandContext,
    tx: Arc<Mutex<S>>,
    rx: Arc<mpsc::Receiver<Option<Message>>>,
) -> Result<rpc::Response, anyhow::Error>
where
    S::Error: std::error::Error,
{
    use rpc::BinaryRequestKind::*;
    use rpc::Request::*;
    use rpc::TextRequest::*;
    use rpc::*;

    match req {
        Binary(req) => match req.kind {
            Init => {
                process.debugger.reset_store();
                let imports = remote_import_module(req.bytes, tx, rx)?;
                for (name, module) in imports {
                    process.debugger.load_host_module(name, module);
                }
                process.debugger.load_module(req.bytes)?;
                return Ok(rpc::Response::Text(TextResponse::Init));
            }
        },
        Text(Version) => {
            return Ok(TextResponse::Version {
                value: VERSION.to_string(),
            }
            .into());
        }
        Text(CallResult { .. }) => unreachable!(),
        Text(CallExported { name, args }) => {
            use wasminspect_debugger::RunResult;
            let func = process.debugger.lookup_func(&name)?;
            let func_ty = process.debugger.func_type(func)?;
            if func_ty.params.len() != args.len() {
                return Err(RequestError::CallArgumentLengthMismatch.into());
            }
            let args = args
                .iter()
                .zip(func_ty.params.iter())
                .map(|(arg, ty)| from_js_number(*arg, ty))
                .collect();
            match process.debugger.execute_func(func, args) {
                Ok(RunResult::Finish(values)) => {
                    let values = values.iter().map(from_vm_wasm_value).collect();
                    return Ok(TextResponse::CallResult { values }.into());
                }
                Ok(RunResult::Breakpoint) => {
                    let mut result = process.run_loop(context)?;
                    loop {
                        match result {
                            CommandResult::ProcessFinish(values) => {
                                let values = values.iter().map(from_vm_wasm_value).collect();
                                return Ok(TextResponse::CallResult { values }.into());
                            }
                            CommandResult::Exit => {
                                match process.dispatch_command("process continue", context)? {
                                    Some(r) => {
                                        result = r;
                                    }
                                    None => {
                                        result = process.run_loop(context)?;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(msg) => {
                    return Err(msg.into());
                }
            }
        }
    }
}
