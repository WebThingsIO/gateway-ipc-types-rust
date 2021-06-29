/**
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.*
 */
use std::{
    fs::File,
    path::{Path, PathBuf},
};

use convert_case::{Case, Casing};
use serde_json::Value;

pub fn generate(path: &Path) -> String {
    let message_schemas = read_message_schemas(path);
    generate_extras(&message_schemas)
}
struct MessageSchema {
    path: PathBuf,
    schema: Value,
}

impl MessageSchema {
    pub fn new(path: PathBuf) -> Self {
        let schema = Self::schema(&path);
        Self { path, schema }
    }

    fn schema(path: &Path) -> Value {
        serde_json::from_reader(
            File::open(path).expect(&format!("Open schema file {}", path.display())),
        )
        .expect(&format!("Parse JSON schema {}", path.display()))
    }

    pub fn name(&self) -> String {
        self.path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_case(Case::Pascal)
    }

    pub fn id(&self) -> i64 {
        self.schema
            .as_object()
            .expect("Schema root is object")
            .get("properties")
            .expect("Schema has properties")
            .as_object()
            .expect("Schema properties is object")
            .get("messageType")
            .expect("Schema has messageType")
            .as_object()
            .expect("Schema messageType is object")
            .get("const")
            .expect("Schema messageType is const")
            .as_i64()
            .expect("Schema messageType is integer")
    }
}

fn read_message_schemas(path: &Path) -> Vec<MessageSchema> {
    let schema: Value = serde_json::from_reader(
        File::open(path).expect(&format!("Open schema file {}", path.display())),
    )
    .expect(&format!("Parse JSON schema {}", path.display()));

    schema
        .as_object()
        .expect("Schema root is object")
        .get("properties")
        .expect("Schema has properties")
        .as_object()
        .expect("Schema properties is object")
        .get("message")
        .expect("Schema has message")
        .as_object()
        .expect("Schema message is object")
        .get("oneOf")
        .expect("Schema has oneOf")
        .as_array()
        .expect("Schema oneOf is array")
        .into_iter()
        .map(|obj| {
            let file = obj
                .as_object()
                .expect("Schema oneOf entry is object")
                .get("$ref")
                .expect("Schema has $ref")
                .as_str()
                .expect("Schema $ref is string");
            MessageSchema::new(path.parent().expect("Path parent").join(file))
        })
        .collect()
}

macro_rules! iterate {
    ($fmt:expr, $files:expr) => {{
        let mut code = "".to_owned();
        for file in $files {
            code += &format!(
                concat!($fmt, "{name:.0}{id:.0}"),
                name = file.name(),
                id = file.id().to_string(),
            );
        }
        code
    }};
}

fn generate_extras(schemas: &Vec<MessageSchema>) -> String {
    format!(
        "
        use std::{{fmt::{{self, Display, Formatter}}, str::FromStr}};

        use serde::{{ser::{{self, Serializer}}, Serialize, Deserialize}};

        use crate::types::*;

        pub trait MessageType {{
            const MESSAGE_ID: i64;
        }}

        pub trait MessageBase {{
            fn plugin_id(&self) -> &str;
            fn message_id(&self) -> i64;
        }}

        #[derive(Serialize, Deserialize, Debug)]
        pub struct GenericMessage {{
            #[serde(rename = \"messageType\")]           
            message_type: i64
        }}

        #[derive(Debug)]
        pub struct Error {{
            message: String,
        }}

        impl Display for Error {{
            fn fmt(&self, f: &mut Formatter) -> fmt::Result {{
                write!(f, \"Cannot parse Message: {{}}\", &self.message)
            }}
        }}

        {schemafy_impl}

        #[derive(Debug)]
        pub enum Message {{
            {message_enum}
        }}

        impl MessageBase for Message {{
            fn message_id(&self) -> i64 {{
                match self {{
                    {message_message_id}
                }}
            }}
            fn plugin_id(&self) -> &str {{
                match self {{
                    {message_plugin_id}
                }}
            }}
        }}
        
        impl FromStr for Message {{
            type Err = Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {{
                let msg: GenericMessage = serde_json::from_str(s)
                    .map_err(|e| 
                        Error {{ message: format!(\"Invalid message: {{}}\", e.to_string()).to_owned() }}
                    )?;
                let code = msg.message_type;
                match code {{
                    {message_from_str}
                    _ => Err(Error {{ message: \"Unknown message type\".to_owned() }}),
                }}
            }}
        }}

        impl ser::Serialize for Message {{
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {{
                match self {{
                    {message_serialize}
                }}
            }}
        }}
        ",
        message_enum = iterate!("{name}({name}),", schemas),
        message_plugin_id = iterate!("Message::{name}(msg) => msg.plugin_id(),", schemas),
        message_message_id = iterate!("Message::{name}(_) => {name}::MESSAGE_ID,", schemas),
        message_serialize = iterate!("Message::{name}(msg) => msg.serialize(serializer),", schemas),
        message_from_str = iterate!(
            "
            {name}::MESSAGE_ID => 
                Ok(Message::{name}(
                    serde_json::from_str(s).map_err(|e| 
                        Error {{ message: format!(\"Invalid JSON: {{}}\", e.to_string()).to_owned() }}
                    )?
                )),
            ",
            schemas
        ),
        schemafy_impl = iterate!(
            "
            impl MessageType for {name} {{
                const MESSAGE_ID: i64 = {id};
            }}
            impl MessageBase for {name} {{
                fn plugin_id(&self) -> &str {{
                    &self.data.plugin_id
                }}
                fn message_id(&self) -> i64 {{
                    Self::MESSAGE_ID
                }}
            }}
            impl Into<{name}> for {name}MessageData {{
                fn into(self) -> {} {{
                    {name} {{
                        data: self,
                        message_type: {name}::MESSAGE_ID,
                    }}
                }}
            }}
            impl Into<Message> for {name} {{
                fn into(self) -> Message {{
                    Message::{name}(self)
                }}
            }}
            impl Into<Message> for {name}MessageData {{
                fn into(self) -> Message {{
                    let msg: {name} = self.into();
                    msg.into()
                }}
            }}
            ",
            schemas
        ),
    )
}
