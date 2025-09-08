use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        tracing: { feature = "tracing" },
        serde: { feature = "serde" },
        client: { feature = "client" },
        anchor: { feature = "anchor" },
        client_traits: { feature = "client-traits" },
        http_rpc_sender: { feature = "http-rpc-sender" },
    }
}
