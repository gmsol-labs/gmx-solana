use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        tracing: { feature = "tracing" },
    }
}
