export default {
  docsRepositoryBase: "https://github.com/darkyzhou/seele/tree/main/docs",
  logo: (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <img src="/logo.svg" width="30px" />
      Seele
    </div>
  ),
  project: {
    link: "https://github.com/darkyzhou/seele",
  },
  search: {
    component: null,
  },
  i18n: [
    { locale: "en", text: "English (WIP)" },
    { locale: "zh", text: "中文" },
  ],
  feedback: {
    content: "文档有问题？欢迎反馈 →",
  },
  editLink: {
    text: "编辑此页面",
  },
  footer: {
    text: (
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          width: "100%",
        }}
      >
        <span>别把看重的东西附上无边的价值</span>
        <span>
          Made with ❤️ by{" "}
          <a
            style={{ textDecoration: "underline" }}
            href="https://darkyzhou.net"
            target="_blank"
            rel="noopener"
          >
            darkyzhou
          </a>
        </span>
      </div>
    ),
  },
};
