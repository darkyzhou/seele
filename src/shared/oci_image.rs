#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct OciImage {
    pub registry: String,
    pub name: String,
    pub tag: String,
}

impl From<&str> for OciImage {
    fn from(value: &str) -> Self {
        const DEFAULT_TAG: &str = "latest";
        const DEFAULT_REGISTRY: &str = "docker.io";

        let (rest, tag) = match value.rfind(':').map(|i| value.split_at(i)) {
            None => (value, DEFAULT_TAG),
            Some((rest, tag)) => (rest, tag.trim_start_matches(':')),
        };

        let (registry, name) = match rest.split_once('/') {
            None => (DEFAULT_REGISTRY, rest),
            Some((registry, name)) => {
                if registry != "localhost" && !registry.contains('.') {
                    (DEFAULT_REGISTRY, rest)
                } else {
                    (registry, name)
                }
            }
        };

        Self { registry: registry.to_string(), name: name.to_string(), tag: tag.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::OciImage;

    macro_rules! image {
        ($registry:expr, $name:expr, $tag:expr) => {
            OciImage {
                registry: $registry.to_string(),
                name: $name.to_string(),
                tag: $tag.to_string(),
            }
        };
    }

    #[test]
    fn test_from_str() {
        let cases = vec![
            (
                "docker.io/rancher/system-upgrade-controller:v0.8.0",
                image!("docker.io", "rancher/system-upgrade-controller", "v0.8.0"),
            ),
            ("busybox:1.34.1-glibc", image!("docker.io", "busybox", "1.34.1-glibc")),
            (
                "rancher/system-upgrade-controller:v0.8.0",
                image!("docker.io", "rancher/system-upgrade-controller", "v0.8.0"),
            ),
            ("127.0.0.1:5000/helloworld:latest", image!("127.0.0.1:5000", "helloworld", "latest")),
            ("quay.io/go/go/gadget:arms", image!("quay.io", "go/go/gadget", "arms")),
            ("busybox", image!("docker.io", "busybox", "latest")),
            ("docker.io/alpine", image!("docker.io", "alpine", "latest")),
            ("library/alpine", image!("docker.io", "library/alpine", "latest")),
        ];

        for (str, image) in cases {
            assert_eq!(OciImage::from(str), image, "case {}", str);
        }
    }
}
