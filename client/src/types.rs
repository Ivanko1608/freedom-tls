pub(crate) trait ProxySender {
    async fn server_start(self) -> anyhow::Result<()>;
}
