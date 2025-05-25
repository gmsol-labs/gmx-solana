use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        tracing: { feature = "tracing" },
        serde: { feature = "serde" },
        js: { feature = "js" },
        treasury: { feature = "treasury" },
        timelock: { feature = "timelock" },
        competition: { feature = "competition" },
    }
}
