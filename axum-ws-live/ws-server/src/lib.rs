use axum::{
    extract::{ws::Message, WebSocketUpgrade},
    response::IntoResponse,
    Extension,
};
use dashmap::{DashMap, DashSet};
use futures::{Sink, SinkExt, Stream, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::log::warn;

use ws_shared::{Msg, MsgData};

const CAPACITY: usize = 64;

#[derive(Debug)]
struct State {
    // for a given user, how many rooms they're in
    user_rooms: DashMap<String, DashSet<String>>,
    // for a given room, how many users are in it
    room_users: DashMap<String, DashSet<String>>,
    tx: broadcast::Sender<Arc<Msg>>,
}

impl Default for State {
    fn default() -> Self {
        let (tx, _rx) = broadcast::channel(CAPACITY);
        Self {
            user_rooms: DashMap::default(),
            room_users: DashMap::default(),
            tx,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ChatState(Arc<State>);

impl ChatState {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn get_user_rooms(&self, username: &str) -> Vec<String> {
        self.0
            .user_rooms
            .get(username)
            .map(|rooms| rooms.clone().into_iter().collect())
            .unwrap_or_default()
    }

    pub fn get_room_users(&self, room: &str) -> Vec<String> {
        self.0
            .room_users
            .get(room)
            .map(|users| users.clone().into_iter().collect())
            .unwrap_or_default()
    }
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Extension(state): Extension<ChatState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket<S>(socket: S, state: ChatState)
where
    S: Stream<Item = Result<Message, axum::Error>> + Sink<Message> + Send + 'static,
{
    let mut rx = state.0.tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    let state1 = state.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(data)) = receiver.next().await {
            match data {
                Message::Text(msg) => {
                    handle_message(msg.as_str().try_into().unwrap(), state1.0.clone()).await;
                }
                _ => (),
            }
        }
    });

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            let data = msg.as_ref().try_into().unwrap();
            if sender.send(Message::Text(data)).await.is_err() {
                warn!("failed to send message");
                break;
            }
        }
    });

    // if any of the tasks failed, we need to shut down the other one
    tokio::select! {
        _ = &mut recv_task => send_task.abort(),
        _ = &mut send_task => recv_task.abort(),
    }

    // this user has left. Should send a leave message to all rooms
    // usually we can get username from auth header, here we just use "fake_user"
    let username = "fake_user";
    warn!("connected for {username} closed");
    for room in state.get_user_rooms(username) {
        if let Err(e) = state.0.tx.send(Arc::new(Msg::leave(&room, username))) {
            warn!("failed to send leave message: {e}");
        }
    }
}

async fn handle_message(msg: Msg, state: Arc<State>) {
    println!("receive: {:?}", msg);
    let msg = match msg.data {
        MsgData::Join => {
            let username = msg.username.clone();
            let room = msg.room.clone();
            state
                .user_rooms
                .entry(username.clone())
                .or_insert_with(DashSet::new)
                .insert(room.clone());
            state
                .room_users
                .entry(room)
                .or_insert_with(DashSet::new)
                .insert(username);
            msg
        }
        MsgData::Leave => {
            if let Some(v) = state.user_rooms.get_mut(&msg.username) {
                v.remove(&msg.room); // 执行该操作会拿锁 v => RwLockGuard<DashSet<String>>
                if v.is_empty() {
                    drop(v); // 先释放掉锁，下面的操作会再次拿锁
                    state.user_rooms.remove(&msg.username);
                }
            }
            if let Some(v) = state.room_users.get_mut(&msg.room) {
                v.remove(&msg.username); // 执行该操作会拿锁 v => RwLockGuard<DashSet<String>>
                if v.is_empty() {
                    drop(v); // 先释放掉锁，下面的操作会再次拿锁
                    state.room_users.remove(&msg.room);
                }
            }
            msg
        }
        _ => msg,
    };

    if let Err(e) = state.tx.send(Arc::new(msg)) {
        warn!("error sending message: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Ok, Result};
    use fake_socket::*;

    #[tokio::test]
    async fn handle_join_should_work() -> Result<()> {
        let (mut _client1, mut _client2, state) = prepare_connections().await?;

        // verify state
        let mut users = state.get_room_users("lobby");
        users.sort();
        assert_eq!(users, vec!["alice", "tyr"]);

        let rooms = state.get_user_rooms("tyr");
        assert_eq!(rooms, &["lobby"]);

        Ok(())
    }

    #[tokio::test]
    async fn handle_message_and_leave_should_work() -> Result<()> {
        let (mut client1, mut client2, state) = prepare_connections().await?;

        let msg1 = &Msg::new("lobby", "tyr", MsgData::Message("hello world".into()));
        client1.send(Message::Text(msg1.try_into()?))?;

        verify(
            &mut client1,
            "lobby",
            "tyr",
            MsgData::Message("hello world".into()),
        )
        .await?;

        verify(
            &mut client2,
            "lobby",
            "tyr",
            MsgData::Message("hello world".into()),
        )
        .await?;

        let msg2 = &Msg::new("lobby", "tyr", MsgData::Leave);
        client2.send(Message::Text(msg2.try_into()?))?;

        // then get alice join msg
        assert!(client1.recv().await.is_some());
        assert!(client2.recv().await.is_some());

        // verify state
        let users = state.get_room_users("lobby");
        assert_eq!(users, vec!["alice"]);

        let rooms = state.get_user_rooms("tyr");
        assert!(rooms.is_empty());

        Ok(())
    }

    async fn prepare_connections() -> Result<(FakeClient<Message>, FakeClient<Message>, ChatState)>
    {
        let (mut client1, socket1) = create_fake_connection();
        let (mut client2, socket2) = create_fake_connection();
        let state = ChatState::new();

        // mimic server hehavior
        let state1 = state.clone();
        tokio::spawn(async move {
            handle_socket(socket1, state1).await;
        });

        let state2 = state.clone();
        tokio::spawn(async move {
            handle_socket(socket2, state2).await;
        });

        let msg1 = &Msg::join("lobby", "tyr");
        client1.send(Message::Text(msg1.try_into()?))?;

        let msg2 = &Msg::join("lobby", "alice");
        client2.send(Message::Text(msg2.try_into()?))?;

        // should first get tyr join msg
        verify(&mut client1, "lobby", "tyr", MsgData::Join).await?;
        verify(&mut client2, "lobby", "tyr", MsgData::Join).await?;

        // then get alice join msg
        assert!(client1.recv().await.is_some());
        assert!(client2.recv().await.is_some());

        Ok((client1, client2, state))
    }

    async fn verify(
        client: &mut FakeClient<Message>,
        room: &str,
        username: &str,
        data: MsgData,
    ) -> Result<()> {
        if let Some(Message::Text(msg)) = client.recv().await {
            let msg: Msg = Msg::try_from(msg.as_str())?;
            assert_eq!(msg.room, room);
            assert_eq!(msg.username, username);
            assert_eq!(msg.data, data);
        }

        Ok(())
    }
}
