use async_trait::async_trait;
use connectorx::{
    constants::RECORD_BATCH_SIZE,
    destinations::arrow::ArrowDestinationError,
    errors::{ConnectorXError, ConnectorXOutError},
    prelude::{get_arrow, ArrowDestination, CXQuery, SourceConn},
};
use core::fmt;
use datafusion::{
    arrow::{
        datatypes::{Field, Schema, SchemaRef},
        record_batch::RecordBatch,
    },
    error::{DataFusionError, Result},
    physical_plan::{stream::RecordBatchStreamAdapter, EmptyRecordBatchStream, RecordBatchStream, SendableRecordBatchStream},
};
use futures::{Stream, StreamExt};
use std::{
    sync::Arc,
    task::{Context, Poll},
};
use tokio::task::{self, JoinError};

pub type SQLExecutorRef = Arc<dyn SQLExecutor>;

#[async_trait]
pub trait SQLExecutor: Sync + Send {
    fn name(&self) -> &str;
    fn compute_context(&self) -> Option<String>;
    // Can use futures::stream::try_unfold to return async stream in sync function
    fn execute(&self, query: &str) -> Result<SendableRecordBatchStream>;
}

impl fmt::Debug for dyn SQLExecutor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {:?}", self.name(), self.compute_context())
    }
}

impl fmt::Display for dyn SQLExecutor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {:?}", self.name(), self.compute_context())
    }
}

// TODO: break out SQLExecutor implementations
pub struct CXExecutor {
    context: String,
    conn: SourceConn,
}

impl CXExecutor {
    pub fn new(dsn: String) -> Result<Self> {
        let conn = SourceConn::try_from(dsn.as_str()).map_err(cx_error_to_df)?;
        Ok(Self { context: dsn, conn })
    }

    pub fn new_with_conn(conn: SourceConn) -> Self {
        Self {
            context: conn.conn.to_string(),
            conn,
        }
    }

    pub fn context(&mut self, context: String) {
        self.context = context;
    }
}

fn cx_error_to_df(err: ConnectorXError) -> DataFusionError {
    DataFusionError::External(format!("ConnectorX: {err:?}").into())
}

#[async_trait]
impl SQLExecutor for CXExecutor {
    fn name(&self) -> &str {
        "connector_x_executor"
    }
    fn compute_context(&self) -> Option<String> {
        Some(self.context.clone())
    }
    fn execute(&self, sql: &str) -> Result<SendableRecordBatchStream> {
        let conn = self.conn.clone();
        let query: CXQuery = sql.into();
        //debug!("CXExecutor Executing SQL: {}", sql);


        let mut dst = get_arrow(&conn, None, &[query.clone()]).map_err(cx_out_error_to_df)?;
        let stream = if let Some(batch) = dst.record_batch().map_err(cx_dst_error_to_df)?{
            futures::stream::once(async move {Ok(batch)})
        } else{
            return Ok(Box::pin(EmptyRecordBatchStream::new(Arc::new(Schema::empty()))))
        };

        let schema = schema_to_lowercase(dst.arrow_schema());
        
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            schema,
            stream,
        )))
    }
}

pub struct ArrowDestinationStream(ArrowDestination);

impl Stream for ArrowDestinationStream {
    type Item = datafusion::error::Result<RecordBatch>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        Poll::Ready({
            let batch = self.0.record_batch().map_err(cx_dst_error_to_df)?;
            batch.map(Ok)
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = RECORD_BATCH_SIZE;
        (size, Some(size))
    }
}

fn cx_dst_error_to_df(err: ArrowDestinationError) -> DataFusionError {
    DataFusionError::External(format!("ConnectorX failed to run query: {err:?}").into())
}

/// Get the schema with lowercase field names
fn schema_to_lowercase(schema: SchemaRef) -> SchemaRef {

    // DF needs lower case schema
    let lower_fields: Vec<_> = schema
        .fields
        .iter()
        .map(|f| {
            Field::new(
                f.name().to_ascii_lowercase(),
                f.data_type().clone(),
                f.is_nullable(),
            )
        })
        .collect();

    Arc::new(Schema::new(lower_fields))
}


fn cx_out_error_to_df(err: ConnectorXOutError) -> DataFusionError {
    DataFusionError::External(format!("ConnectorX failed to run query: {err:?}").into())
}

fn join_error_to_df(err: JoinError) -> DataFusionError {
    DataFusionError::External(format!("task failed: {err:?}").into())
}
