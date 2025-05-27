use serde::{
    ser::{Impossible, SerializeMap, SerializeStruct},
    Serialize, Serializer,
};

pub mod types {
    use std::default;

    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Room {
        // The UUID of the room
        pub id: String,
        // Number of users currently in the room
        pub current: i32,
        // Max number of users for the room
        pub max: i32,
        // Name of the room
        pub name: String,
        // Whether the room is current in game
        pub ingame: bool,
        // Set if the room has a password
        pub password: bool,
        // Token that can be used to join a room without a password
        pub join_token: Option<String>,
    }

    #[derive(Debug, Default, Serialize, Deserialize)]
    #[serde(default)]
    pub struct User {
        // The UUID of this user
        pub id: String,
        // The display name of the user
        pub name: String,
        // Whether the user is ready or not
        pub ready: bool,
        // Whether the user is missing the song or not
        pub missing_map: bool,
        // User's selected level for the current song<br/>
        // 0 = no selected level
        pub level: Option<i32>,
        // User's last final score<br/>
        // If no score, this field will not be present
        pub score: Option<i32>,
        // User's last max combo<br/>
        // If no score, this field will not be present
        pub combo: Option<i32>,
        // User's clear status<br/>
        // 0: Exited<br/>
        // 1: Failed<br/>
        // 2: Clear<br/>
        // 3: Hard Clear<br/>
        // 4: Full Combo<br/>
        // 5: Perfect<br/>
        // If no score, this field will not be present
        pub clear: Option<u8>,
        // Exists if the user has provided any extra data.
        pub extra_data: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct GameScore {
        pub id: String,
        pub name: String,
        pub score: i32,
    }
}

pub mod client {
    use super::types;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Joined {
        pub room: types::Room,
    }

    pub enum RoomResult {}

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Info {
        // Version of the server
        pub version: String,
        // UUID of the current user
        pub userid: String,
        // The refresh rate of the scoreboard (how often to send scores)
        pub refresh_rate: i32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Rooms {
        pub rooms: Option<Vec<types::Room>>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Error {
        pub error: String,
    }

    pub enum Server {
        Info(Info),
        Rooms(Rooms),
        Room(RoomResult),
        Error(Error),
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(tag = "topic")]
    pub enum ClientCommand {
        //Server
        #[serde(rename = "server.info")]
        Info(Info),
        #[serde(rename = "server.rooms")]
        Rooms(Rooms),
        #[serde(rename = "server.error")]
        Error(Error),

        //Server Room
        #[serde(rename = "server.room.joined")]
        Joined(Joined),
        #[serde(rename = "server.room.badpassword")]
        BadPassword {},

        //Room
        #[serde(rename = "room.update")]
        RoomUpdate(Update),

        //Game
        #[serde(rename = "game.started")]
        Started(Started),
        #[serde(rename = "game.scoreboard")]
        Scoreboard(Scoreboard),
        #[serde(rename = "game.sync.start")]
        SyncStart(Start),
    }

    #[derive(Debug, Default, Serialize, Deserialize)]
    #[serde(default)]
    pub struct Update {
        pub users: Vec<types::User>,
        pub do_rotate: bool,
        pub start_soon: bool,
        pub song: Option<String>,
        pub diff: Option<u32>,
        pub level: Option<u32>,
        pub hash: Option<String>,
        pub audio_hash: Option<String>,
        pub chart_hash: Option<String>,
        pub host: String,
        pub hard_mode: bool,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Start {
        pub users: Vec<types::User>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Started {
        pub hard: bool,
        pub mirror: bool,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Scoreboard {
        pub users: Vec<types::GameScore>,
    }
}

struct TopicExtractor(Option<String>);

#[allow(unused)]
impl Serializer for TopicExtractor {
    type Ok = Option<String>;

    type Error = std::fmt::Error;

    type SerializeSeq = Impossible<Self::Ok, Self::Error>;

    type SerializeTuple = Impossible<Self::Ok, Self::Error>;

    type SerializeTupleStruct = Impossible<Self::Ok, Self::Error>;

    type SerializeTupleVariant = Impossible<Self::Ok, Self::Error>;

    type SerializeMap = Self;

    type SerializeStruct = Self;

    type SerializeStructVariant = Impossible<Self::Ok, Self::Error>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(Some(v.to_owned()))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        Err(std::fmt::Error)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_newtype_struct<T>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        Err(std::fmt::Error)
    }

    fn serialize_newtype_variant<T>(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        Err(std::fmt::Error)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(std::fmt::Error)
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(TopicExtractor(None))
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(std::fmt::Error)
    }
}

impl SerializeStruct for TopicExtractor {
    type Ok = Option<String>;

    type Error = std::fmt::Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        if key == "topic" {
            self.0 = value.serialize(TopicExtractor(None))?;
        }

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.0)
    }
}

impl SerializeMap for TopicExtractor {
    type Ok = Option<String>;

    type Error = std::fmt::Error;

    fn serialize_key<T>(&mut self, _: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        unreachable!()
    }

    fn serialize_value<T>(&mut self, _: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + serde::Serialize,
    {
        unreachable!()
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.0)
    }
    fn serialize_entry<K, V>(&mut self, key: &K, value: &V) -> Result<(), Self::Error>
    where
        K: ?Sized + serde::Serialize,
        V: ?Sized + serde::Serialize,
    {
        if self.0.is_some() {
            return Ok(());
        }

        let key = key.serialize(TopicExtractor(None))?;

        if key.is_some_and(|x| x == "topic") {
            self.0 = value.serialize(TopicExtractor(None))?;
        }

        Ok(())
    }
}

pub fn get_topic(v: impl Serialize) -> Option<String> {
    v.serialize(TopicExtractor(None)).ok()?
}

pub mod server {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Join {
        pub id: Option<String>,
        pub password: Option<String>,
        pub token: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct New {
        pub name: String,
        pub password: Option<String>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Auth {
        pub password: String,
        pub name: String,
        pub version: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Level {
        pub level: u32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Extra {
        pub data: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SetSong {
        /// The folder name of the selected song
        pub song: String,
        /// The index of the chosen difficulty
        pub diff: i32,
        /// The level of the chosen difficulty
        pub level: i32,
        /// Selective hash of the song file (0x8000 bytes) *Deprecated
        pub hash: String,
        /// Selective hash of the song file (0x8000 bytes) *Deprecated
        pub audio_hash: String,
        /// Normalized hash of chart file (see USC for more details)
        pub chart_hash: String,
    }

    impl SetSong {
        pub const NAUTICA_PATH: &'static str = "__nautica__";
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "topic")]
    pub enum ServerCommand {
        // Server
        #[serde(rename = "server.rooms")]
        Rooms,

        // Server room
        #[serde(rename = "server.room.join")]
        RoomJoin(Join),
        #[serde(rename = "server.room.new")]
        RoomNew(New),
        #[serde(rename = "room.sync.ready")]
        Sync,

        // User
        #[serde(rename = "user.auth")]
        Auth(Auth),
        #[serde(rename = "user.ready.toggle")]
        Ready,
        #[serde(rename = "user.mirror.toggle")]
        Mirror,
        #[serde(rename = "user.hard.toggle")]
        Hard,
        #[serde(rename = "user.song.level")]
        Level(Level),
        #[serde(rename = "user.nomap")]
        Nomap {},

        #[serde(rename = "user.extra.set")]
        Extra(Extra),

        // Room
        #[serde(rename = "room.leave")]
        Leave,
        #[serde(rename = "room.game.start")]
        StartGame,
        #[serde(rename = "room.option.rotation.toggle")]
        ToggleHostRotate,
        #[serde(rename = "room.setsong")]
        SetSong(SetSong),
        #[serde(rename = "room.score.update")]
        ScoreUpdate {
            time: i32,
            score: i32,
        },
        #[serde(rename = "room.score.final")]
        ScoreFinal {
            score: i32,
            combo: i32,
            clear: u8,
        },
        #[serde(rename = "room.update.get")]
        GetUpdate,
        #[serde(rename = "room.set.host")]
        SetHost {
            host: String,
        },
        Raw(String),
    }
}
