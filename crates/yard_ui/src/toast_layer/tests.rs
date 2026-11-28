use super::ToastKind;
use gpui::Role;

#[test]
fn toast_kind_a11y_role_maps_warning_and_error_to_alert_and_info_to_status() {
    let info: Role = ToastKind::Info.a11y_role();
    assert_eq!(info, Role::Status);
    assert_eq!(ToastKind::Warning.a11y_role(), Role::Alert);
    assert_eq!(ToastKind::Error.a11y_role(), Role::Alert);
}
