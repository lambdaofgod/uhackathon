use std::collections::HashMap;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{Index, IndexWriter};

pub struct TantivyIndexWrapper {
    index: Index,
    schema: Schema,
    title_field_name: String,
}

impl TantivyIndexWrapper {
    pub fn new(index_name: String, name_field: String, fields: Vec<String>) -> Self {
        let mut schema_builder = Schema::builder();
        schema_builder.add_text_field(&name_field.to_string(), TEXT | STORED);
        for field in fields {
            schema_builder.add_text_field(&field.to_string(), TEXT);
        }
        let schema = schema_builder.build();
        let index = Index::create_in_ram(schema.clone());
        TantivyIndexWrapper {
            index,
            schema,
            title_field_name: name_field,
        }
    }

    pub fn field_names(&self) -> Vec<&str> {
        let fields = self.schema.fields();

        fields
            .map(|(field, _)| self.schema.get_field_name(field))
            .collect::<Vec<_>>()
    }

    pub fn num_docs(&self) -> tantivy::Result<u64> {
        let reader = self.index.reader()?;
        Ok(reader.searcher().num_docs())
    }

    // TODO: accept a hashmap
    pub fn add_document(&self, document_map: HashMap<&str, String>) -> tantivy::Result<()> {
        let mut index_writer = self.index.writer(50_000_000)?;
        self.add_document_nocommit(&index_writer, document_map)?;
        index_writer.commit()?;
        Ok(())
    }

    pub fn add_document_nocommit(
        &self,
        index_writer: &IndexWriter,
        document_map: HashMap<&str, String>,
    ) -> tantivy::Result<()> {
        let doc = Document::from(self.field_vec_from_hashmap(document_map));
        index_writer.add_document(doc);
        Ok(())
    }

    pub fn search(&self, query: &str) -> tantivy::Result<Vec<String>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let title_field = self.schema.get_field(&self.title_field_name).unwrap();
        let fields = self
            .schema
            .fields()
            .map(|(field, _)| field)
            .collect::<Vec<_>>();
        let query_parser = QueryParser::for_index(&self.index, fields);
        let query = query_parser.parse_query(query)?;
        let top_docs = searcher.search(&query, &TopDocs::with_limit(10))?;
        let mut results = Vec::new();
        for (_score, doc_address) in top_docs {
            let retrieved_doc = searcher.doc(doc_address)?;
            let title = retrieved_doc
                .get_first(title_field)
                .unwrap()
                .as_text()
                .unwrap();
            results.push(title.to_string());
        }
        Ok(results)
    }

    fn field_vec_from_hashmap(&self, doc: HashMap<&str, String>) -> Vec<FieldValue> {
        let mut field_vec = Vec::new();
        for (key, value) in doc {
            let field_res = self.schema.get_field(key);
            match field_res {
                Ok(field) => {
                    field_vec.push(FieldValue::new(field, Value::Str(value)));
                }
                Err(_) => {}
            }
        }
        field_vec
    }
}

pub struct BatchIndexer<'a> {
    index_writer: IndexWriter,
    index: &'a TantivyIndexWrapper,
}

impl<'a> BatchIndexer<'a> {
    pub fn new(index: &'a TantivyIndexWrapper) -> Self {
        let index_writer = index.index.writer(2_000_000_000).unwrap();
        BatchIndexer {
            index_writer: index_writer,
            index: index,
        }
    }

    pub fn add_document(&self, document_map: HashMap<&str, String>) -> tantivy::Result<()> {
        self.index
            .add_document_nocommit(&self.index_writer, document_map)?;
        Ok(())
    }

    pub fn commit(&mut self) -> tantivy::Result<()> {
        self.index_writer.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_init_index() {
        let index = super::TantivyIndexWrapper::new(
            "test_index".to_string(),
            "title".to_string(),
            vec!["body".to_string()],
        );
        let n_fields = index.schema.num_fields();
        assert_eq!(n_fields, 2);
    }

    #[test]
    fn test_indexing() {
        let index = super::TantivyIndexWrapper::new(
            "test_index".to_string(),
            "title".to_string(),
            vec!["body".to_string()],
        );
        let doc_map = vec![
            ("title".to_string(), "test title".to_string()),
            ("body".to_string(), "test body".to_string()),
        ]
        .iter()
        .cloned()
        .collect::<std::collections::HashMap<_, _>>();

        index.add_document(doc_map).unwrap();
        let num_docs = index.num_docs().unwrap();
        assert_eq!(num_docs, 1);
    }
}
