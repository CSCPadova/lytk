//! Named values: IR enums written, in JSON and on display, as their name in
//! the IR's vocabulary (MusicXML's, where MusicXML has a word for it).

/// An enum of named values from one table: `as_str`, `from_name`, and
/// serde as the name (an unknown name is an error).
macro_rules! named_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        $vis enum $name {
            $($(#[$vmeta])* #[serde(rename = $text)] $variant,)+
        }

        impl $name {
            /// The value's name.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }

            /// The value called `name`, if there is one.
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

/// [`named_enum`] for an open vocabulary: any other name is kept, as
/// `Other`, so every name reads (`From<&str>`) and writes back as given.
macro_rules! open_named_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(from = "String", into = "String")]
        $vis enum $name {
            $($(#[$vmeta])* $variant,)+
            /// Any other name, as written.
            Other(String),
        }

        impl $name {
            /// The value's name.
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $text,)+
                    Self::Other(name) => name,
                }
            }
        }

        impl From<&str> for $name {
            fn from(name: &str) -> Self {
                match name {
                    $($text => Self::$variant,)+
                    _ => Self::Other(name.to_string()),
                }
            }
        }

        impl From<String> for $name {
            fn from(name: String) -> Self {
                Self::from(name.as_str())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.as_str().to_string()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
