mod polars_documents;
mod wrappers;
use clap::Parser;
use polars::prelude::{CsvReader, PolarsResult};
use polars::prelude::{PolarsError, SerReader};
use polars_documents::{df_rows_foreach, IndexableCollection};
use std::default::Default;
use std::fs::File;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{doc, Index, IndexWriter, ReloadPolicy};
use tempfile::TempDir;
use wrappers::TantivyIndexWrapper;

fn tantivy_index_example_impl() -> tantivy::Result<()> {
    // Let's create a temporary directory for the
    // sake of this example
    let index_path = TempDir::new()?;

    // # Defining the schema
    //
    // The Tantivy index requires a very strict schema.
    // The schema declares which fields are in the index,
    // and for each field, its type and "the way it should
    // be indexed".

    // First we need to define a schema ...
    let mut schema_builder = Schema::builder();

    // Our first field is title.
    // We want full-text search for it, and we also want
    // to be able to retrieve the document after the search.
    //
    // `TEXT | STORED` is some syntactic sugar to describe
    // that.
    //
    // `TEXT` means the field should be tokenized and indexed,
    // along with its term frequency and term positions.
    //
    // `STORED` means that the field will also be saved
    // in a compressed, row-oriented key-value store.
    // This store is useful for reconstructing the
    // documents that were selected during the search phase.
    schema_builder.add_text_field("title", TEXT | STORED);

    // Our second field is body.
    // We want full-text search for it, but we do not
    // need to be able to be able to retrieve it
    // for our application.
    //
    // We can make our index lighter by omitting the `STORED` flag.
    schema_builder.add_text_field("body", TEXT);

    let schema = schema_builder.build();

    // # Indexing documents
    //
    // Let's create a brand new index.
    //
    // This will actually just save a meta.json
    // with our schema in the directory.
    let index = Index::create_in_dir(&index_path, schema.clone())?;

    // To insert a document we will need an index writer.
    // There must be only one writer at a time.
    // This single `IndexWriter` is already
    // multithreaded.
    //
    // Here we give tantivy a budget of `50MB`.
    // Using a bigger memory_arena for the indexer may increase
    // throughput, but 50 MB is already plenty.
    let mut index_writer: IndexWriter = index.writer(50_000_000)?;

    // Let's index our documents!
    // We first need a handle on the title and the body field.

    // ### Adding documents
    //
    // We can create a document manually, by setting the fields
    // one by one in a Document object.
    let title = schema.get_field("title").unwrap();
    let body = schema.get_field("body").unwrap();

    let mut old_man_doc = Document::default();
    old_man_doc.add_text(title, "The Old Man and the Sea");
    old_man_doc.add_text(
        body,
        "He was an old man who fished alone in a skiff in the Gulf Stream and he had gone \
         eighty-four days now without taking a fish.",
    );

    // ... and add it to the `IndexWriter`.
    index_writer.add_document(old_man_doc)?;

    // For convenience, tantivy also comes with a macro to
    // reduce the boilerplate above.
    index_writer.add_document(doc!(
    title => "Of Mice and Men",
    body => "A few miles south of Soledad, the Salinas River drops in close to the hillside \
            bank and runs deep and green. The water is warm too, for it has slipped twinkling \
            over the yellow sands in the sunlight before reaching the narrow pool. On one \
            side of the river the golden foothill slopes curve up to the strong and rocky \
            Gabilan Mountains, but on the valley side the water is lined with trees—willows \
            fresh and green with every spring, carrying in their lower leaf junctures the \
            debris of the winter’s flooding; and sycamores with mottled, white, recumbent \
            limbs and branches that arch over the pool"
    ))?;

    // Multivalued field just need to be repeated.
    index_writer.add_document(doc!(
    title => "Frankenstein",
    title => "The Modern Prometheus",
    body => "You will rejoice to hear that no disaster has accompanied the commencement of an \
             enterprise which you have regarded with such evil forebodings.  I arrived here \
             yesterday, and my first task is to assure my dear sister of my welfare and \
             increasing confidence in the success of my undertaking."
    ))?;

    // This is an example, so we will only index 3 documents
    // here. You can check out tantivy's tutorial to index
    // the English wikipedia. Tantivy's indexing is rather fast.
    // Indexing 5 million articles of the English wikipedia takes
    // around 3 minutes on my computer!

    // ### Committing
    //
    // At this point our documents are not searchable.
    //
    //
    // We need to call `.commit()` explicitly to force the
    // `index_writer` to finish processing the documents in the queue,
    // flush the current index to the disk, and advertise
    // the existence of new documents.
    //
    // This call is blocking.
    index_writer.commit()?;

    // If `.commit()` returns correctly, then all of the
    // documents that have been added are guaranteed to be
    // persistently indexed.
    //
    // In the scenario of a crash or a power failure,
    // tantivy behaves as if it has rolled back to its last
    // commit.

    // # Searching
    //
    // ### Searcher
    //
    // A reader is required first in order to search an index.
    // It acts as a `Searcher` pool that reloads itself,
    // depending on a `ReloadPolicy`.
    //
    // For a search server you will typically create one reader for the entire lifetime of your
    // program, and acquire a new searcher for every single request.
    //
    // In the code below, we rely on the 'ON_COMMIT' policy: the reader
    // will reload the index automatically after each commit.
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommit)
        .try_into()?;

    // We now need to acquire a searcher.
    //
    // A searcher points to a snapshotted, immutable version of the index.
    //
    // Some search experience might require more than
    // one query. Using the same searcher ensures that all of these queries will run on the
    // same version of the index.
    //
    // Acquiring a `searcher` is very cheap.
    //
    // You should acquire a searcher every time you start processing a request and
    // and release it right after your query is finished.
    let searcher = reader.searcher();

    // ### Query

    // The query parser can interpret human queries.
    // Here, if the user does not specify which
    // field they want to search, tantivy will search
    // in both title and body.
    let query_parser = QueryParser::for_index(&index, vec![title, body]);

    // `QueryParser` may fail if the query is not in the right
    // format. For user facing applications, this can be a problem.
    // A ticket has been opened regarding this problem.
    let query = query_parser.parse_query("sea whale")?;

    // A query defines a set of documents, as
    // well as the way they should be scored.
    //
    // A query created by the query parser is scored according
    // to a metric called Tf-Idf, and will consider
    // any document matching at least one of our terms.

    // ### Collectors
    //
    // We are not interested in all of the documents but
    // only in the top 10. Keeping track of our top 10 best documents
    // is the role of the `TopDocs` collector.

    // We can now perform our query.
    let top_docs = searcher.search(&query, &TopDocs::with_limit(10))?;

    // The actual documents still need to be
    // retrieved from Tantivy's store.
    //
    // Since the body field was not configured as stored,
    // the document returned will only contain
    // a title.
    println!("Retrieved {} docs", top_docs.len());
    for (_score, doc_address) in top_docs {
        let retrieved_doc: Document = searcher.doc(doc_address)?;
        for field in retrieved_doc.field_values() {
            println!("{}", field.value().as_text().unwrap());
        }
    }

    // We can also get an explanation to understand
    // how a found document got its score.
    let query = query_parser.parse_query("title:sea^20 body:whale^70")?;

    let (_score, doc_address) = searcher
        .search(&query, &TopDocs::with_limit(1))?
        .into_iter()
        .next()
        .unwrap();

    let explanation = query.explain(&searcher, doc_address)?;

    println!("The explanation is :");
    println!("{}", explanation.to_pretty_json());

    Ok(())
}

fn tantivy_result_to_std<T>(res: tantivy::Result<T>) -> Result<T, String> {
    match res {
        Ok(v) => Ok(v),
        Err(e) => Err(format!("{}", e)),
    }
}

fn tantivy_index_example() -> Result<(), String> {
    tantivy_result_to_std(tantivy_index_example_impl())
}

fn polars_example(csv_path: &str) -> PolarsResult<()> {
    let reader = CsvReader::from_path(csv_path).unwrap();

    let df = reader.has_header(true).finish()?;

    df_rows_foreach::<PolarsError>(&df, &|row| {
        println!("{:?}", row);
        Ok(())
    })
}

fn polars_result_to_result<T>(res: PolarsResult<T>) -> Result<T, String> {
    match res {
        Ok(x) => Ok(x),
        Err(e) => Err(format!("{}", e)),
    }
}

fn polars_search_example(csv_path: String, query: String) -> Result<(), String> {
    let reader = CsvReader::from_path(&csv_path).unwrap();

    let df_res = reader.has_header(true).finish();
    let df = polars_result_to_result(df_res)?;

    let index = TantivyIndexWrapper::new(
        "test_index".to_string(),
        "repo".to_string(),
        vec!["dependencies".to_string()],
    );

    let indexing_result = df.index_collection(&index);

    match index.search(&query) {
        Ok(search_result) => {
            println!("Search result: {:?}", search_result);
            Ok(())
        }
        Err(e) => Err(format!("Error: {}", e)),
    }
}

#[derive(clap::ValueEnum, Debug, Clone)]
enum ExampleType {
    Tantivy,
    Polars,
    PolarsSearchExample,
}

#[derive(Parser, Debug)]
struct Args {
    #[clap(value_enum, default_value = "polars-search-example")]
    example_type: ExampleType,
    #[clap(default_value = "data/search_example_small.csv")]
    csv_path: Option<String>,
    #[clap(default_value = "text")]
    query: String,
}

fn main() -> Result<(), String> {
    let args = Args::parse();

    match args.example_type {
        ExampleType::Tantivy => tantivy_index_example(),
        ExampleType::Polars => polars_result_to_result(polars_example(&args.csv_path.unwrap())),
        ExampleType::PolarsSearchExample => {
            polars_search_example(args.csv_path.unwrap(), args.query)
        }
    }
}
