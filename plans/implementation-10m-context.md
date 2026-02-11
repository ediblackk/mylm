Architecture review complete. The document is at [`docs/architecture-10m-context.md`](docs/architecture-10m-context.md).

**Key recommendations answering your 5 questions:**

1. **Chunking**: Use 10k-20k token chunks (not 100k) with 10% overlap + semantic boundary snapping. 100k chunks are too large for meaningful embedding—BGE-Small saturates around 512 tokens, so you need smaller chunks with hierarchical grouping above them.

2. **Storage**: 15GB in LanceDB is fine, but you **must** build an IVF_PQ index (256 partitions) or queries degrade to brute-force scan. Add `chunk_index`, `parent_doc_id`, `section_path` fields to the Arrow schema. Batch writes through a queue to avoid write contention.

3. **Retrieval**: Vector-only is insufficient at 10M scale. Use **hybrid search**: vector similarity (top-200) + BM25 via `tantivy` (top-200) → reciprocal rank fusion → cross-encoder re-rank (top-10). This catches both semantic and lexical matches.

4. **Workers**: **Stateless** with shared [`VectorStore`](core/src/memory/store.rs:86) access. Your existing [`DelegateTool`](core/src/agent/tools/delegate.rs:129) already shares `memory_store`—add a semaphore-based pool (4-8 concurrent). Stateful workers waste RAM holding chunks that LanceDB already indexes.

5. **Meta-question**: No, you're not reimplementing LanceDB. LanceDB handles storage/indexing. You're building the **retrieval orchestration layer** on top: chunking, hierarchical indexing, hybrid search, query expansion, and cross-boundary awareness. LanceDB is your storage engine, not your retrieval strategy.

**The Awareness Problem solution**: Build a 3-tier hierarchical index:
- **L0**: 10k-token chunks (embeddings)
- **L1**: Section summaries (LLM-generated, ~500 tokens each)  
- **L2**: Document summary (single embedding)

Query flow: search L0 for direct hits → expand to L1 siblings → if query references multiple sections, search L2 summaries first to identify relevant sections, then drill into L0. This solves "compare section 1 with section 89" by finding both sections at L1/L2 level.

**What bites at 10M+ scale**: Embedding throughput (27h on single CPU thread for 10M tokens), write contention on concurrent ingestion, context window bloat when injecting too many retrieved chunks, and stale index after incremental updates.