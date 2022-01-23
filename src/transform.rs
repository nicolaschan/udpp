use std::{collections::{HashMap, BinaryHeap, VecDeque}, iter, sync::Arc};

use crate::veq::VeqError;
use tokio::sync::Mutex;
use tokio_stream::{self as stream, StreamExt};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait Bidirectional {
    async fn send(&mut self, data: Vec<u8>) -> Result<(), VeqError>;
    async fn recv(&mut self) -> Result<Vec<u8>, VeqError>;
}

#[async_trait]
impl Bidirectional for VecDeque<Vec<u8>> {
    async fn send(&mut self, data: Vec<u8>) -> Result<(), VeqError> {
        self.push_back(data);
        Ok(())
    }
    async fn recv(&mut self) -> Result<Vec<u8>, VeqError> {
        Ok(self.pop_front().unwrap())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkedData {
    group: u64,
    index: u64,
    group_length: u64,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct ChunkGroup {
    chunks:  Vec<Option<ChunkedData>>,
    size: u64,
    group_length: u64,
}

impl ChunkGroup {
    pub fn new(group_length: u64) -> ChunkGroup {
        ChunkGroup {
            chunks: iter::repeat(None).take(group_length as usize).collect(),
            size: 0,
            group_length,
        }
    }

    pub fn add_chunk(&mut self, chunk: ChunkedData) {
        self.chunks.remove(chunk.index as usize);
        self.chunks.insert(chunk.index as usize, Some(chunk));
        self.size += 1;
    }

    pub fn is_complete(&self) -> bool {
        self.size == self.group_length 
    }

    pub fn collect(self) -> Vec<u8> {
        let mut output = Vec::new();
        for chunk in self.chunks {
            output.extend(chunk.unwrap().data);
        }
        return output;
    }
}

#[derive(Clone)]
pub struct Chunker<T> {
    delegate: T,
    chunk_size: u64,
    current_group: Arc<Mutex<u64>>,
    chunk_groups: Arc<Mutex<HashMap<u64, ChunkGroup>>>,
    completed_chunks: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

impl<T> Chunker<T> {
    pub fn new(delegate: T, chunk_size: u64) -> Chunker<T> {
        Chunker {
            delegate,
            chunk_size,
            current_group: Arc::new(Mutex::new(0)),
            chunk_groups: Arc::new(Mutex::new(HashMap::new())),
            completed_chunks: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub async fn add_chunk(&mut self, chunk: ChunkedData) {
        let group_id = chunk.group;
        let is_complete = {
            let mut chunk_groups_guard = self.chunk_groups.lock().await;
            let group = chunk_groups_guard
                .entry(chunk.group)
                .or_insert_with(|| ChunkGroup::new(chunk.group_length));
            group.add_chunk(chunk);
            group.is_complete()
        };
        if is_complete {
            let g = self.chunk_groups.lock().await.remove(&group_id).unwrap();
            self.completed_chunks.lock().await.push_back(g.collect());
        }
    }

    pub async fn next_ready_chunk(&mut self) -> Option<Vec<u8>> {
        self.completed_chunks.lock().await.pop_front()
    }
}

#[async_trait]
impl<T: Bidirectional + Send> Bidirectional for Chunker<T> {
    async fn send(&mut self, data: Vec<u8>) -> Result<(), VeqError> {
        let chunks = data.chunks(self.chunk_size as usize);
        let group_length = chunks.len() as u64;
        let group = self.current_group.lock().await.clone();

        let mut current_group_guard = self.current_group.lock().await;
        *current_group_guard = group + 1;

        let data: Vec<Vec<u8>> = chunks.into_iter()
            .map(|s| s.to_vec())
            .enumerate()
            .map(|(index, data)| ChunkedData {
                group, index: index as u64, group_length, data
            })
            .map(|chunked| bincode::serialize(&chunked).unwrap())
            .collect();
        
        for d in data {
            self.delegate.send(d).await?;
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<Vec<u8>, VeqError> {
        loop {
            match self.next_ready_chunk().await {
                Some(ready) => return Ok(ready),
                None => {
                    let bytes = self.delegate.recv().await.unwrap();
                    let chunked_data: ChunkedData = bincode::deserialize(&bytes).unwrap();
                    self.add_chunk(chunked_data).await;
                },
            };
        }
    }
}


#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

    use tokio::{join, sync::Mutex};
    use uuid::Uuid;

    use crate::{veq::{VeqSocket, VeqSession}, transform::Chunker};

    use super::Bidirectional;

    #[tokio::test]
    async fn test_send_recv() {
        let delegate: VecDeque<Vec<u8>> = VecDeque::new();
        let mut chunker = Chunker::new(delegate, 3);

        let data = vec![1,2,3,4,5,6,7,8];
        chunker.send(data.clone()).await.unwrap();
        let received = chunker.recv().await.unwrap();
        assert_eq!(data, received);
    }
}