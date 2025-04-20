use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        tracing: { feature = "tracing" },
        serde: { feature = "serde" },
        client: { feature = "client" },
        anchor: { feature = "anchor" },
    }
}
