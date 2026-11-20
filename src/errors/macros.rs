macro_rules! define_error {
    (
        $error_enum:ident {
            $(
                $enum_variant:ident $( ( $error_type:ty ) )? => {
                    code: $code:expr,
                    client_msg: $client_msg:expr
                    $(, display_msg: $display_msg:expr )?
                    $(,)?
                }
            ),* $(,)?
        }
    ) => {
        #[derive(Debug)]
        pub enum $error_enum {
            $(
                $enum_variant $( ($error_type) )?,
            )*
        }

        impl ErrorMeta for $error_enum {
            const LOCAL_CODES: &'static [u16] = &[ $( $code ),* ];
        }

        impl RegisteredError for $error_enum {

            fn local_code(&self) -> u16 {
                match self {
                    $(
                        Self::$enum_variant $( (define_error!(@ignore $error_type)) )? => $code,
                    )*
                }
            }

            fn client_message(&self) -> String {
                match self {
                    $(
                        Self::$enum_variant $( (define_error!(@bind value, $error_type)) )? => {
                            define_error!(@render $client_msg; value $(, $error_type)?)
                        },
                    )*
                }
            }
        }

        impl std::fmt::Display for $error_enum {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(
                        Self::$enum_variant $( (define_error!(@bind value, $error_type)) )? => {
                            write!(f, "{}", define_error!(@render define_error!(@select $client_msg $(, $display_msg)?); value $(, $error_type)?))
                        },
                    )*
                }
            }
        }

        impl std::error::Error for $error_enum {}
    };

    (@ignore $i:ty) => { _ };
    (@bind $name:ident, $ty:ty) => { $name };

    (@select $default:expr, $custom:expr) => {
        $custom
    };

    (@select $default:expr) => {
        $default
    };

    (@render $msg:expr; $name:ident, $ty:ty) => {
        ($msg)($name)
    };

    (@render $msg:expr; $name:ident) => {
        $msg.to_string()
    };
}
