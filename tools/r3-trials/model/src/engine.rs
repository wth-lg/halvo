use std::ffi::{c_char, c_void, CStr, CString};
use std::num::NonZeroU32;
use std::path::Path;
use std::time::Instant;

use llama_cpp_2::context::params::{KvCacheType, LlamaContextParams};
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use crate::prompt;
use crate::settings::Settings;
use crate::wire::{Grammar, Reply, Request};

pub const THREADS: u32 = 4;
pub const BATCH: u32 = 2048;
pub const UBATCH: u32 = 512;
const CACHE: KvCacheType = KvCacheType::F16;
const ACTION_GRAMMAR: &str = include_str!("../grammars/action.gbnf");

/// The largest tensor that llama.cpp of llama-cpp-sys-2 0.1.161 reads into memory in every
/// load mode: its lazy mode `auto` maps a marked tensor above this size whatever the load
/// mode, and llama-cpp-2 0.1.161 has no switch for that mode.
pub const LARGEST_READ_TENSOR: u64 = 4 * 1024 * 1024 * 1024;

unsafe extern "C" fn drop_log(_level: u32, _text: *const c_char, _data: *mut c_void) {}

/// Drops llama.cpp's log while the child checks its inputs, so that a refusal prints one line.
pub fn quiet_log() {
    unsafe { llama_cpp_sys_2::llama_log_set(Some(drop_log), std::ptr::null_mut()) }
}

/// Gives llama.cpp's log back to its default, standard error.
pub fn default_log() {
    unsafe { llama_cpp_sys_2::llama_log_set(None, std::ptr::null_mut()) }
}

/// The name and byte size of each tensor of a GGUF file, read from its header alone.
pub fn tensor_sizes(path: &Path) -> Result<Vec<(String, u64)>, String> {
    let text = path
        .to_str()
        .ok_or_else(|| "the path of the weights is not UTF-8".to_string())?;
    if !path.is_file() {
        return Err(format!("{text} is no file"));
    }
    let name = CString::new(text).map_err(|_| "the path of the weights holds a NUL".to_string())?;
    let params = llama_cpp_sys_2::gguf_init_params {
        no_alloc: true,
        ctx: std::ptr::null_mut(),
    };
    let ctx = unsafe { llama_cpp_sys_2::gguf_init_from_file(name.as_ptr(), params) };
    if ctx.is_null() {
        return Err(format!("{text} is no GGUF file"));
    }
    let count = unsafe { llama_cpp_sys_2::gguf_get_n_tensors(ctx) };
    let mut sizes = Vec::new();
    for id in 0..count {
        let size = unsafe { llama_cpp_sys_2::gguf_get_tensor_size(ctx, id) } as u64;
        let tensor = unsafe { CStr::from_ptr(llama_cpp_sys_2::gguf_get_tensor_name(ctx, id)) }
            .to_string_lossy()
            .into_owned();
        sizes.push((tensor, size));
    }
    unsafe { llama_cpp_sys_2::gguf_free(ctx) };
    Ok(sizes)
}

/// Why a file cannot be loaded under the loader rules, or `None`: a tensor above
/// [`LARGEST_READ_TENSOR`] would be mapped, and a mapped page is in no reading of a peak.
pub fn tensor_fault(sizes: &[(String, u64)]) -> Option<String> {
    sizes
        .iter()
        .find(|(_, bytes)| *bytes > LARGEST_READ_TENSOR)
        .map(|(name, bytes)| {
            format!("the tensor {name} has {bytes} bytes: llama.cpp maps a tensor above {LARGEST_READ_TENSOR} bytes whatever the load mode")
        })
}

/// Starts llama.cpp and refuses a build that can offload to a GPU.
pub fn backend() -> Result<LlamaBackend, String> {
    let backend = LlamaBackend::init().map_err(|e| format!("start llama.cpp: {e}"))?;
    if backend.supports_gpu_offload() {
        return Err("this build of llama.cpp can offload to a GPU: build in the directory of the crate, where .cargo/config.toml turns Metal off".to_string());
    }
    Ok(backend)
}

/// Loads the weights with no layer on a GPU and llama.cpp's load mode none: the weights are
/// read into the process and not mapped.
pub fn load_model(backend: &LlamaBackend, path: &Path) -> Result<LlamaModel, String> {
    let params = LlamaModelParams::default()
        .with_n_gpu_layers(0)
        .with_use_mmap(false)
        .with_use_mlock(false);
    if params.n_gpu_layers() != 0 || params.use_mmap() || params.use_mlock() {
        return Err(
            "the model parameters did not take GPU layers 0 and load mode none".to_string(),
        );
    }
    LlamaModel::load_from_file(backend, path, &params).map_err(|e| format!("load the model: {e}"))
}

/// One context of the settings' size, with the batch sizes and threads of this child, a key
/// and value cache of type f16 and no offload of the cache or of operations.
pub fn new_context<'m>(
    backend: &LlamaBackend,
    model: &'m LlamaModel,
    settings: &Settings,
) -> Result<LlamaContext<'m>, String> {
    let ctx = NonZeroU32::new(settings.context)
        .ok_or_else(|| "the context of the settings is 0".to_string())?;
    let params = LlamaContextParams::default()
        .with_n_ctx(Some(ctx))
        .with_n_batch(BATCH)
        .with_n_ubatch(UBATCH)
        .with_n_threads(THREADS as i32)
        .with_n_threads_batch(THREADS as i32)
        .with_type_k(CACHE)
        .with_type_v(CACHE)
        .with_offload_kqv(false)
        .with_op_offload(false);
    let context = model
        .new_context(backend, params)
        .map_err(|e| format!("create the context: {e}"))?;
    let in_force = (context.n_ctx(), context.n_batch(), context.n_ubatch());
    if in_force != (ctx.get(), BATCH, UBATCH) {
        return Err(format!(
            "llama.cpp set context, batch and micro-batch to {in_force:?}, not to {:?}",
            (ctx.get(), BATCH, UBATCH)
        ));
    }
    Ok(context)
}

fn templated(model: &LlamaModel, system: &str, user: &str) -> Result<String, String> {
    let message = |role: &str, content: &str| {
        LlamaChatMessage::new(role.to_string(), content.to_string())
            .map_err(|e| format!("a chat message: {e}"))
    };
    let chat = [message("system", system)?, message("user", user)?];
    let template = model
        .chat_template(None)
        .map_err(|e| format!("the model's chat template: {e}"))?;
    model
        .apply_chat_template(&template, &chat, true)
        .map_err(|e| format!("apply the model's chat template: {e}"))
}

fn count_tokens(model: &LlamaModel, text: &str) -> usize {
    model.vocab().tokenize(text.as_bytes(), false, false).len()
}

/// Answers one request: the system text and the user turn through the model's chat
/// template, greedy sampling at temperature 0, the action grammar or free text, and at most
/// the level's token limit.
pub fn answer(
    model: &LlamaModel,
    context: &mut LlamaContext<'_>,
    settings: &Settings,
    request: &Request,
) -> Result<Reply, String> {
    let start = Instant::now();
    let vocab = model.vocab();
    let snapshot = request.snapshot.as_deref().map(|text| {
        prompt::cut_snapshot(text, settings.snapshot_tokens as usize, |part| {
            count_tokens(model, part)
        })
    });
    let user = prompt::user_text(&request.prompt, snapshot);
    let text = templated(model, &request.system, &user)?;
    let mut tokens = vocab.tokenize(text.as_bytes(), true, true);
    let bos = vocab.bos();
    if tokens.len() >= 2 && tokens[0] == bos && tokens[1] == bos {
        tokens.remove(0);
    }
    let limits = settings.limits(request.level);
    let limit = match request.grammar {
        Grammar::Action => limits.action_tokens,
        Grammar::Text => limits.reply_tokens,
    } as usize;
    let n_ctx = context.n_ctx() as usize;
    if tokens.is_empty() || tokens.len() + limit > n_ctx {
        return Err(format!(
            "{} prompt tokens and {limit} reply tokens do not fit the context of {n_ctx}",
            tokens.len()
        ));
    }
    let mut sampler = match request.grammar {
        Grammar::Action => LlamaSampler::chain_simple([
            LlamaSampler::grammar(model, ACTION_GRAMMAR, "root")
                .map_err(|e| format!("the action grammar: {e}"))?,
            LlamaSampler::greedy(),
        ]),
        Grammar::Text => LlamaSampler::chain_simple([LlamaSampler::greedy()]),
    };
    context.clear_kv_cache();
    let batch_size = context.n_batch() as usize;
    let mut batch = LlamaBatch::new(batch_size, 1);
    let last = tokens.len() - 1;
    let mut position = 0i32;
    for (chunk, part) in tokens.chunks(batch_size).enumerate() {
        batch.clear();
        for (offset, token) in part.iter().enumerate() {
            let logits = chunk * batch_size + offset == last;
            batch
                .add(*token, position, &[0], logits)
                .map_err(|e| format!("fill the batch: {e}"))?;
            position += 1;
        }
        context
            .decode(&mut batch)
            .map_err(|e| format!("decode the prompt: {e}"))?;
    }
    let mut first_token_ms = None;
    let mut out: Vec<u8> = Vec::new();
    let mut tokens_out = 0u64;
    for _ in 0..limit {
        let token: LlamaToken = sampler.sample(context, -1);
        first_token_ms.get_or_insert_with(|| start.elapsed().as_millis() as u64);
        if vocab.is_eog(token) {
            break;
        }
        tokens_out += 1;
        out.extend(vocab.token_to_piece(token, false, None));
        batch.clear();
        batch
            .add(token, position, &[0], true)
            .map_err(|e| format!("fill the batch: {e}"))?;
        position += 1;
        context
            .decode(&mut batch)
            .map_err(|e| format!("decode a reply token: {e}"))?;
    }
    let ms = start.elapsed().as_millis() as u64;
    Ok(Reply {
        id: request.id,
        text: String::from_utf8_lossy(&out).into_owned(),
        tokens_in: tokens.len() as u64,
        tokens_out,
        first_token_ms: first_token_ms.unwrap_or(ms),
        ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn a_tensor_above_four_gibibytes_is_refused() {
        let at = vec![("a".to_string(), 4 * GIB), ("b".to_string(), 1)];
        assert_eq!(tensor_fault(&at), None);
        assert_eq!(tensor_fault(&[]), None);
        let above = vec![
            ("a".to_string(), 1),
            ("per_layer_token_embd.weight".to_string(), 4 * GIB + 1),
        ];
        assert_eq!(
            tensor_fault(&above),
            Some("the tensor per_layer_token_embd.weight has 4294967297 bytes: llama.cpp maps a tensor above 4294967296 bytes whatever the load mode".to_string())
        );
    }
}
