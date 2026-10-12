use super::*;

#[test]
fn regional_routes_round_trip() {
    for vendor in [
        Vendor::Qwen,
        Vendor::Moonshot,
        Vendor::MiniMax,
        Vendor::Zhipu,
    ] {
        assert_eq!(vendor.endpoints().len(), 2);
        for &(region, endpoint) in vendor.endpoints() {
            assert_eq!(Vendor::from_endpoint(endpoint), Some(vendor));
            assert_eq!(Vendor::from_endpoint(&format!("{endpoint}/")), Some(vendor));
            assert_eq!(vendor.region(endpoint), Some(region));
        }
    }
    assert!(Vendor::DeepSeek.endpoints().is_empty());
    assert_eq!(Vendor::DeepSeek.endpoint(), "https://api.deepseek.com");
}

#[test]
fn coding_routes_round_trip() {
    for vendor in [
        Vendor::Qwen,
        Vendor::Moonshot,
        Vendor::MiniMax,
        Vendor::Zhipu,
        Vendor::Doubao,
    ] {
        assert!(vendor.coding_model().is_some());
        for &(region, endpoint) in vendor.coding_endpoints() {
            assert_eq!(Vendor::from_endpoint(endpoint), Some(vendor));
            assert_eq!(vendor.region(endpoint), Some(region));
        }
    }
    assert_eq!(Vendor::Doubao.coding_endpoints().len(), 1);
    assert_eq!(Vendor::Doubao.coding_endpoints()[0].0, Region::China);
    assert_eq!(
        Vendor::MiniMax.coding_endpoints(),
        Vendor::MiniMax.endpoints()
    );
}

#[test]
fn legacy_minimax_domains_remain_recognized() {
    for endpoint in ["https://api.minimax.chat/v1", "https://api.minimaxi.com/v1"] {
        assert_eq!(Vendor::from_endpoint(endpoint), Some(Vendor::MiniMax));
    }
    assert_eq!(
        Vendor::MiniMax.region("https://api.minimaxi.com/v1"),
        Some(Region::China)
    );
    assert_eq!(Vendor::MiniMax.region("https://api.minimax.chat/v1"), None);
    assert_eq!(Vendor::MiniMax.endpoint(), "https://api.minimax.cn/v1");
}
