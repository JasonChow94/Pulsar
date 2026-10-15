use super::registry;

const VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
/// Windows 11 still calls itself Windows 10 in `ProductName`.
const WINDOWS_11_BUILD: u32 = 22000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OsInfo {
    pub name: String,
    pub version: Option<String>,
    pub build: Option<String>,
}

pub fn product_name(registry_name: &str, build: u32) -> String {
    if build >= WINDOWS_11_BUILD {
        registry_name.replacen("Windows 10", "Windows 11", 1)
    } else {
        registry_name.to_string()
    }
}

pub fn read() -> OsInfo {
    let build_number: Option<u32> =
        registry::string(VERSION_KEY, "CurrentBuildNumber").and_then(|b| b.parse().ok());
    let ubr = registry::dword(VERSION_KEY, "UBR");
    OsInfo {
        name: product_name(
            &registry::string(VERSION_KEY, "ProductName").unwrap_or_else(|| "Windows".into()),
            build_number.unwrap_or(0),
        ),
        version: registry::string(VERSION_KEY, "DisplayVersion"),
        build: build_number.map(|b| match ubr {
            Some(ubr) => format!("{b}.{ubr}"),
            None => b.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_11_is_named_by_its_build() {
        assert_eq!(product_name("Windows 10 Pro", 26100), "Windows 11 Pro");
        assert_eq!(product_name("Windows 10 Pro", 19045), "Windows 10 Pro");
    }

    #[test]
    fn this_machine_has_a_build() {
        assert!(read().build.is_some());
    }
}
