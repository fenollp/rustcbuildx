pub trait Popped: Clone {
    #[must_use]
    fn pop(&mut self) -> bool;
    fn popped(&mut self, times: usize) -> Self
    where
        Self: Sized,
    {
        for _ in 0..times {
            assert!(self.pop());
        }
        self.clone()
    }
}

impl Popped for camino::Utf8PathBuf {
    #[must_use]
    fn pop(&mut self) -> bool {
        self.pop()
    }
}

pub trait ShowCmd {
    #[must_use]
    fn show(&self) -> String;
}

impl ShowCmd for std::process::Command {
    #[must_use]
    fn show(&self) -> String {
        format!(
            "`{command} {args}`",
            command = self.get_program().to_string_lossy(),
            args = self
                .get_args()
                .map(|x| x.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

impl ShowCmd for tokio::process::Command {
    #[must_use]
    fn show(&self) -> String {
        self.as_std().show()
    }
}
