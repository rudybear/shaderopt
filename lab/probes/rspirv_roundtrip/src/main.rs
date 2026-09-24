use rspirv::binary::Assemble;
fn main() {
    let path = std::env::args().nth(1).expect("spv path");
    let bytes = std::fs::read(&path).expect("read");
    let words: Vec<u32> = bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0],c[1],c[2],c[3]])).collect();
    let module = rspirv::dr::load_words(&words).expect("load");
    let n_types = module.types_global_values.len();
    let n_funcs = module.functions.len();
    let n_inst: usize = module.functions.iter().map(|f| f.blocks.iter().map(|b| b.instructions.len()).sum::<usize>()).sum();
    let out = module.assemble();
    let same = out == words; let body_same = out[5..] == words[5..]; let hdr_in: Vec<String> = words[..5].iter().map(|w| format!("{:#x}", w)).collect(); let hdr_out: Vec<String> = out[..5].iter().map(|w| format!("{:#x}", w)).collect();
    let first_diff = out.iter().zip(words.iter()).position(|(a,b)| a != b);
    println!("body_identical_after_header={} hdr_in={:?} hdr_out={:?} words_in={} words_out={} byte_identical={} first_diff_word={:?} header_bound={:?} funcs={} globals={} body_insts={}",
        body_same, hdr_in, hdr_out, words.len(), out.len(), same, first_diff, module.header.as_ref().map(|h| h.bound), n_funcs, n_types, n_inst);
}
