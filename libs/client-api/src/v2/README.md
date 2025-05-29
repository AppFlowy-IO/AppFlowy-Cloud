# AppFlowy-Cloud Client-Server Synchronization

This document describes the synchronization mechanism between the AppFlowy client and server using WebSocket connections
and collaborative real-time updates.

## Architecture Overview

The synchronization system consists of:

- **Client-side**: `WorkspaceController` and `WorkspaceControllerActor` (libs/client-api/src/v2/)
- **Server-side**: WebSocket actors including `WsSession`, `Workspace`, and `WsServer` (
  services/appflowy-collaborate/src/ws2/)

## Two-Client Synchronization Scenario

This diagram shows how two clients (Client A and Client B) with different `client_id`s collaborate on the same document,
including specific message types and payloads. It demonstrates a realistic scenario where Client B has previously
contributed to the document.

```mermaid
sequenceDiagram
    participant CA as Client A (client_id: 12345)
    participant WCA as WorkspaceControllerActor A
    participant WSA as WebSocket A
    participant WSS as WsSession Server
    participant WSP as Workspace Actor
    participant CS as CollabStore
    participant WSB as WebSocket B
    participant WCB as WorkspaceControllerActor B
    participant CB as Client B (client_id: 67890)
    Note over CA, CB: Both clients connect to same workspace
    CA ->> WCA: connect(access_token)
    CB ->> WCB: connect(access_token)
    WCA ->> WSA: establish_connection()
    WCB ->> WSB: establish_connection()
    WSA ->> WSS: WebSocket handshake
    WSB ->> WSS: WebSocket handshake
    WSS ->> WSP: Join { session_id: 12345, workspace_id, uid: 100 }
    WSS ->> WSP: Join { session_id: 67890, workspace_id, uid: 200 }
    Note over CA, CB: Client A opens/creates a document
    CA ->> WCA: bind_and_cache_collab_ref(doc_123, Document)
    WCA ->> WCA: Setup observers for doc_123
    WCA ->> WSA: ClientMessage::Manifest {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> state_vector: [12345:0],<br/> last_message_id: 0<br/>}
    WSA ->> WSS: Binary(Manifest)
    WSS ->> WSP: WsInput {<br/> message: InputMessage::Manifest(Document, rid:0, StateVector([12345:0])),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: get_latest_state(workspace_id, doc_123, [12345:0])

    alt Document exists (with previous updates)
        Note over CS: Server reconstructs document:<br/>- Base document state<br/>- Apply 5 updates from client 12345<br/>- Apply 3 updates from client 67890<br/>- Result: state_vector [12345:5, 67890:3]
        CS -->> WSP: State { update: [existing_data], state_vector: [12345:5, 67890:3] }
        WSP ->> WSS: ServerMessage::Update {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> flags: Lib0v1,<br/> last_message_id: 1001,<br/> update: [existing_data]<br/>}
        WSP ->> WSS: ServerMessage::Manifest {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> last_message_id: 0,<br/> state_vector: [12345:5, 67890:3]<br/>}
    else New document
        WSP ->> WSS: ServerMessage::Manifest {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> last_message_id: 0,<br/> state_vector: []<br/>}
    end

    WSS ->> WSA: Binary(Update + Manifest)
    WSA ->> WCA: Receive remote update
    WCA ->> WCA: save_remote_update() performs multiple steps:
    Note over WCA: 1. Apply update to yrs document with Rid as origin<br/>2. Prune pending updates (check for missing)<br/>3. Set sync state based on result
    alt No missing updates detected
        WCA ->> WCA: Set sync state to SyncFinished
        Note over WCA: Observer automatically saves to disk<br/>Client A state vector: [12345:6, 67890:4]
    else Missing updates detected
        WCA ->> WCA: publish_manifest() to request missing updates
        WCA ->> WCA: Set sync state to Syncing
    end
    WCA ->> CA: UI updated - "Hello World from Client B" appears
    Note over CA, CB: Client B opens the same document (fresh session)
    CB ->> WCB: bind_and_cache_collab_ref(doc_123, Document)
    WCB ->> WCB: Setup observers for doc_123
    Note over WCB: Client B starts fresh with [67890:0]<br/>even though it previously made 3 updates
    WCB ->> WSB: ClientMessage::Manifest {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> state_vector: [67890:0],<br/> last_message_id: 0<br/>}
    WSB ->> WSS: Binary(Manifest)
    WSS ->> WSP: WsInput {<br/> message: InputMessage::Manifest(Document, rid:0, StateVector([67890:0])),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: get_latest_state(workspace_id, doc_123, [67890:0])
    Note over CS: Server has stored updates:<br/>- 5 updates from client 12345<br/>- 3 updates from client 67890<br/>Server computes diff from [67890:0]
    CS -->> WSP: State { update: [full_doc_state], state_vector: [12345:5, 67890:3] }
    WSP ->> WSS: ServerMessage::Update {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> flags: Lib0v1,<br/> last_message_id: 1001,<br/> update: [full_doc_state]<br/>}
    WSP ->> WSS: ServerMessage::Manifest {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> state_vector: [12345:5, 67890:3]<br/>}
    WSS ->> WSB: Binary(Update)
    WSB ->> WCB: Receive remote update
    WCB ->> WCB: save_remote_update() performs multiple steps:
    Note over WCB: 1. Apply update to yrs document with Rid as origin<br/>2. Prune pending updates (check for missing)<br/>3. Set sync state based on result
    alt No missing updates detected
        WCB ->> WCB: Set sync state to SyncFinished
        Note over WCB: Observer automatically saves to disk<br/>Client B state vector: [12345:6, 67890:3]
    else Missing updates detected
        WCB ->> WCB: publish_manifest() to request missing updates
        WCB ->> WCB: Set sync state to Syncing
    end
    WCB ->> CB: UI updated - "Hello World" appears
    Note over CA, CB: Client A makes changes to document
    CA ->> CA: User types "Hello World" at position 0
    CA ->> WCA: yrs update triggered (observer)
    WCA ->> WCA: Generate update: insert("Hello World", pos:0, client:12345)
    WCA ->> WCA: Save to local DB with rid: 1002
    WCA ->> WSA: ClientMessage::Update {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> flags: Lib0v1,<br/> update: [insert_hello_world]<br/>}
    WSA ->> WSS: Binary(Update)
    WSS ->> WSP: WsInput {<br/> message: InputMessage::Update(Document, Update([insert_hello_world])),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: publish_update(workspace_id, doc_123, update, origin: client_12345)
    CS ->> CS: Store update with message_id: 1003
    CS ->> CS: Broadcast to UpdateStreamMessage
    Note over CS: State vector now: [12345:6, 67890:3]
    Note over CA, CB: Server broadcasts to other clients
    CS ->> WSP: UpdateStreamMessage {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> update: [insert_hello_world],<br/> last_message_id: 1003,<br/> sender: client_12345<br/>}
    WSP ->> WSP: Filter sessions (exclude sender client_12345)
    WSP ->> CS: Check read permission for client_67890
    CS -->> WSP: Permission: Read granted
    WSP ->> WSS: ServerMessage::Update {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> flags: Lib0v1,<br/> last_message_id: 1003,<br/> update: [insert_hello_world]<br/>}
    WSS ->> WSB: Binary(Update)
    WSB ->> WCB: Receive remote update
    WCB ->> WCB: Apply update to doc_123 yrs document
    WCB ->> WCB: Save remote update to local DB
    Note over WCB: Client B state vector: [12345:6, 67890:3]
    WCB ->> CB: UI updated - "Hello World" appears
    Note over CA, CB: Client B makes concurrent changes
    CB ->> CB: User types " from Client B" at end
    CB ->> WCB: yrs update triggered (observer)
    WCB ->> WCB: Generate update: insert(" from Client B", pos:11, client:67890)
    WCB ->> WCB: Save to local DB with rid: 1004
    WCB ->> WSB: ClientMessage::Update {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> flags: Lib0v1,<br/> update: [insert_from_client_b]<br/>}
    WSB ->> WSS: Binary(Update)
    WSS ->> WSP: WsInput {<br/> message: InputMessage::Update(Document, Update([insert_from_client_b])),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: publish_update(workspace_id, doc_123, update, origin: client_67890)
    CS ->> CS: Store update with message_id: 1005
    CS ->> CS: Broadcast to UpdateStreamMessage
    Note over CS: State vector now: [12345:6, 67890:4]
    CS ->> WSP: UpdateStreamMessage {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> update: [insert_from_client_b],<br/> last_message_id: 1005,<br/> sender: client_67890<br/>}
    WSP ->> WSP: Filter sessions (exclude sender client_67890)
    WSP ->> CS: Check read permission for client_12345
    CS -->> WSP: Permission: Read granted
    WSP ->> WSS: ServerMessage::Update {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> flags: Lib0v1,<br/> last_message_id: 1005,<br/> update: [insert_from_client_b]<br/>}
    WSS ->> WSA: Binary(Update)
    WSA ->> WCA: Receive remote update
    WCA ->> WCA: Apply update to doc_123 yrs document
    WCA ->> WCA: Save remote update to local DB
    Note over WCA: Client A state vector: [12345:6, 67890:4]
    WCA ->> CA: UI updated - "Hello World from Client B" appears
    Note over CA, CB: Awareness updates (cursor positions)
    CA ->> CA: User moves cursor to position 5
    CA ->> WCA: Awareness change: { cursor: 5, selection: null }
    WCA ->> WSA: ClientMessage::AwarenessUpdate {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> awareness: [client_12345_cursor_pos_5]<br/>}
    WSA ->> WSS: Binary(AwarenessUpdate)
    WSS ->> WSP: WsInput {<br/> message: InputMessage::AwarenessUpdate(AwarenessUpdate),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: publish_awareness_update(workspace_id, doc_123, update)
    CS ->> CS: Broadcast to AwarenessStreamUpdate
    CS ->> WSP: AwarenessStreamUpdate {<br/> object_id: doc_123,<br/> data: [client_12345_cursor_pos_5],<br/> sender: client_12345<br/>}
    WSP ->> WSP: Filter sessions (exclude sender client_12345)
    WSP ->> WSS: ServerMessage::AwarenessUpdate {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> awareness: [client_12345_cursor_pos_5]<br/>}
    WSS ->> WSB: Binary(AwarenessUpdate)
    WSB ->> WCB: Receive awareness update
    WCB ->> CB: Show Client A's cursor at position 5
    Note over CA, CB: Client A disconnects and reconnects
    WSA --x WSA: Connection lost
    WCA ->> WCA: Detect disconnection, trigger reconnection
    Note over CB: Client B continues editing while A is offline
    CB ->> CB: User adds "!!!" at end
    CB ->> WCB: yrs update triggered
    WCB ->> WSB: ClientMessage::Update {<br/> object_id: doc_123,<br/> update: [insert_exclamation]<br/>}
    WSB ->> WSS: Binary(Update)
    WSS ->> WSP: WsInput {<br/> message: InputMessage::Update(Document, Update([insert_exclamation])),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: publish_update (stored as message_id: 1007)
    Note over CS: State vector now: [12345:6, 67890:5]
    Note over CA, CB: Client A reconnects and catches up
    WCA ->> WSA: Reconnect with last_message_id: 1005
    WSA ->> WSS: WebSocket handshake
    WSS ->> WSP: Join { session_id: 12345, last_message_id: 1005 }
    Note over WSP, CS: Server retrieves updates based on Rid
    WSP ->> CS: get_collabs_created_since(workspace_id, timestamp_1005)
    Note over CS: Convert Rid 1005 to timestamp:<br/>Extract timestamp from "1703123456005-0"<br/>Query DB for new collabs since that time
    CS -->> WSP: New collabs: [] (none created)
    WSP ->> CS: get_workspace_updates(workspace_id, since: Rid(1005))
    Note over CS: Redis XRANGE query:<br/>XRANGE af:u:workspace_123 1703123456005-0 +<br/>Returns all updates after Rid 1005
    CS -->> WSP: Missing updates: [<br/> UpdateStreamMessage {<br/> last_message_id: Rid(1007),<br/> object_id: doc_123,<br/> sender: client_67890,<br/> update: [insert_exclamation]<br/> }<br/>]
    Note over WSP: Server processes missing updates
    loop For each missing update
        WSP ->> WSP: Check permissions for client_12345
        WSP ->> WSS: ServerMessage::Update {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> flags: Lib0v1,<br/> last_message_id: 1007,<br/> update: [insert_exclamation]<br/>}
    end

    WSS ->> WSA: Binary(Update)
    WSA ->> WCA: Apply missed update
    WCA ->> WCA: Update local last_message_id to 1007
    Note over WCA: Client A final state vector: [12345:6, 67890:5]<br/>Client A last_message_id: 1007
    WCA ->> CA: UI updated - "Hello World from Client B!!!" appears
    Note over CA, CB: Both clients now have consistent state
    Note over WCA: Client A state: "Hello World", vector: [12345:7, 67890:3]
    Note over WCB: Client B state: "Hello World", vector: [12345:7, 67890:3]
    Note over CA, CB: Client A sends dependent updates
    CA ->> CA: User types "Good " at position 12
    CA ->> WCA: yrs update triggered (observer)
    WCA ->> WCA: Generate Update A: insert("Good ", pos:12, client:12345, clock:8)
    WCA ->> WCA: Save Update A to local DB with rid: 3001
    WCA ->> WSA: ClientMessage::Update {<br/> object_id: doc_123,<br/> update: [Update_A_insert_good]<br/>}
    CA ->> CA: User types "Morning" at position 17 (depends on Update A)
    CA ->> WCA: yrs update triggered (observer)
    WCA ->> WCA: Generate Update B: insert("Morning", pos:17, client:12345, clock:9)
    WCA ->> WCA: Save Update B to local DB with rid: 3002
    WCA ->> WSA: ClientMessage::Update {<br/> object_id: doc_123,<br/> update: [Update_B_insert_morning]<br/>}
    Note over CA, CB: TCP/WebSocket guarantees in-order delivery from same client
    WSA ->> WSS: Binary(Update A) - arrives first (TCP ordering)
    WSS ->> WSP: WsInput {<br/> message: InputMessage::Update(Document, Update_A),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: publish_update(workspace_id, doc_123, Update_A, origin: client_12345)
    CS ->> CS: Store in Redis stream with ID: 1703123456010-0
    CS ->> CS: Broadcast UpdateStreamMessage
    WSA ->> WSS: Binary(Update B) - arrives second (TCP ordering)
    WSS ->> WSP: WsInput {<br/> message: InputMessage::Update(Document, Update_B),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: publish_update(workspace_id, doc_123, Update_B, origin: client_12345)
    CS ->> CS: Store in Redis stream with ID: 1703123456011-0
    CS ->> CS: Broadcast UpdateStreamMessage
    Note over CA, CB: Server broadcasts Update A to Client B
    CS ->> WSP: UpdateStreamMessage {<br/> last_message_id: 1703123456010-0,<br/> object_id: doc_123,<br/> sender: client_12345,<br/> update: [Update_A_insert_good]<br/>}
    WSP ->> WSP: Filter sessions (exclude sender client_12345)
    WSP ->> WSS: ServerMessage::Update {<br/> object_id: doc_123,<br/> last_message_id: 1703123456010-0,<br/> update: [Update_A_insert_good]<br/>}
    WSS ->> WSB: Binary(Update A)
    WSB ->> WCB: Receive Update A
    WCB ->> WCB: Apply Update A to yrs document (insert "Good " at pos:12)
    WCB ->> WCB: Save Update A to local DB
    Note over WCB: Client B state vector: [12345:8, 67890:3]<br/>Document content: "Hello World Good"
    WCB ->> CB: UI updated - "Hello World Good" appears
    Note over CA, CB: Network interruption causes Client B to miss Update B broadcast
    CS ->> WSP: UpdateStreamMessage {<br/> last_message_id: 1703123456011-0,<br/> object_id: doc_123,<br/> sender: client_12345,<br/> update: [Update_B_insert_morning]<br/>}
    WSP ->> WSP: Filter sessions (exclude sender client_12345)
    WSP ->> WSS: ServerMessage::Update {<br/> object_id: doc_123,<br/> last_message_id: 1703123456011-0,<br/> update: [Update_B_insert_morning]<br/>}
    WSS --x WSB: Binary(Update B) - LOST due to network interruption
    Note over CA, CB: Client B detects missing update through heartbeat/reconnection
    WCB ->> WCB: Connection established - publish_pending_collabs() triggered
    WCB ->> WCB: Generate state vector for doc_123: [12345:8, 67890:3] (missing Update B)
    WCB ->> WSB: ClientMessage::Manifest {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> last_message_id: 1703123456010-0,<br/> state_vector: [12345:8, 67890:3]<br/>}
    WSB ->> WSS: Binary(Manifest - requesting missing updates)
    Note over CA, CB: Server provides missing Update B
    WSS ->> WSP: WsInput {<br/> message: InputMessage::Manifest(Document, rid:1703123456010-0, StateVector([12345:8, 67890:3])),<br/> workspace_id, object_id: doc_123, sender, client_id<br/>}
    WSP ->> CS: get_full_collab(workspace_id, doc_123, state_vector:[12345:8, 67890:3])
    Note over CS: Server reconstructs document state:<br/>1. Get all updates from Redis stream<br/>2. Apply in stream order: [Update_A, Update_B]<br/>3. Generate diff for client's state vector
    CS ->> CS: XRANGE af:u:workspace_123 - + (get all updates)
    CS ->> CS: Apply updates in stream order: [Update_A, Update_B]
    CS ->> CS: encode_diff_v1(from: [12345:8, 67890:3]) produces [Update_B]
    CS -->> WSP: EncodedCollab { state_vector: [12345:9, 67890:3], diff: [Update_B] }
    WSP ->> WSS: ServerMessage::Update {<br/> object_id: doc_123,<br/> collab_type: Document,<br/> flags: Lib0v1,<br/> last_message_id: 1703123456011-0,<br/> update: [Update_B_insert_morning]<br/>}
    WSS ->> WSB: Binary(Update B - missing update delivered)
    WSB ->> WCB: Receive missing Update B
    WCB ->> WCB: Apply Update B (insert "Morning" at pos:17)
    WCB ->> WCB: Save Update B to local DB
    Note over WCB: Client B final state vector: [12345:9, 67890:3]<br/>Document content: "Hello World Good Morning"
    WCB ->> CB: UI updated - "Hello World Good Morning" appears
    Note over CA, CB: Final state - both clients consistent after missing update recovery
    Note over WCA: Client A state: "Hello World Good Morning", vector: [12345:9, 67890:3]
    Note over WCB: Client B state: "Hello World Good Morning", vector: [12345:9, 67890:3]
```
