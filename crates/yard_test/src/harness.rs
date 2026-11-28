use gpui::{AnyWindowHandle, Global, Menu, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::Value;
use std::{cell::RefCell, fs, path::PathBuf, rc::Rc, time::Duration};
use tempfile::TempDir;
use yard_app::{Application, Backends, Init, OpenUrls};
use yard_paths::Paths;
use yard_product::Product;
use yard_shell::{SecretStore, Shell, TrayBackend};

const DEFAULT_APPLICATION: Application = Application {
    product: Product {
        name: "Test App",
        version: "0.0.0",
        identifier: "test.yard.app",
    },
    url_scheme: "yardtest",
    update_url: "",
    window_icon: None,
};

pub struct TestAppBuilder {
    application: Application,
    extensions: Vec<(&'static str, Init)>,
    menus: fn(&Product) -> Vec<Menu>,
    settings: Value,
}

impl TestAppBuilder {
    pub fn application(mut self, application: Application) -> Self {
        self.application = application;
        self
    }

    pub fn extension(mut self, name: &'static str, init: Init) -> Self {
        self.extensions.push((name, init));
        self
    }

    pub fn menus(mut self, menus: fn(&Product) -> Vec<Menu>) -> Self {
        self.menus = menus;
        self
    }

    pub fn settings(mut self, settings: Value) -> Self {
        self.settings = settings;
        self
    }

    pub fn boot(self, cx: &mut TestAppContext) -> TestApp {
        cx.update(|cx| {
            assert!(
                !cx.has_global::<BootStarted>(),
                "TestApp::boot requires a fresh TestAppContext"
            );
            cx.set_global(BootStarted);
        });
        let _group =
            tracing::info_span!("test-app-boot", extensions = self.extensions.len()).entered();
        let home = TestHome(Rc::new(
            TempDir::new().expect("create test application home"),
        ));
        let home_path = home.path();
        fs::create_dir_all(&home_path).expect("create test application home");
        let paths = Paths::under(&home_path);
        tracing::debug!("home created");
        save_settings(&paths, &self.settings);
        tracing::debug!("settings written");

        let tray_backend = TrayBackend::memory();
        let window = cx.update(|cx| {
            let product = self.application.product;
            cx.set_app_identity(product.identifier, product.name);
            tracing::debug!(
                identifier = product.identifier,
                name = product.name,
                "app identity set"
            );
            yard_app::init_with_backends(
                cx,
                self.application,
                &paths,
                &self.extensions,
                self.menus,
                &RefCell::new(OpenUrls::default()),
                Backends {
                    secret_store: Some(SecretStore::memory()),
                    tray_backend: Some(tray_backend.clone()),
                },
            );
            let window = yard_app::open_window(cx, self.application, None, None)
                .downcast::<Shell>()
                .expect("application opened a Shell window");
            cx.set_global(HomeRoot {
                _dir: home.0.clone(),
            });
            cx.on_app_quit(move |_| {
                let home_path = home_path.clone();
                async move {
                    let _ = fs::remove_dir_all(home_path);
                }
            })
            .detach();
            window
        });

        let app = TestApp {
            paths,
            window,
            tray_backend,
            _home: home,
        };
        app.draw(cx);
        tracing::debug!("window drawn");
        app
    }
}

/// An isolated headless application. Boot one `TestApp` per test on a fresh GPUI test context.
/// A context is used as soon as boot begins, including when initialization panics.
/// Boot before capturing logs for a single operation. GPUI's test context has no asset source,
/// so icons and images from `yard_ui`'s bundled assets cannot load.
///
/// If a test panics with a data request still queued, the database worker may recreate the
/// test home under the system's temporary directory after the test ends.
pub struct TestApp {
    paths: Paths,
    window: WindowHandle<Shell>,
    tray_backend: TrayBackend,
    _home: TestHome,
}

impl TestApp {
    pub fn builder() -> TestAppBuilder {
        TestAppBuilder {
            application: DEFAULT_APPLICATION,
            extensions: Vec::new(),
            menus: |_| Vec::new(),
            settings: serde_json::json!({}),
        }
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn window(&self) -> WindowHandle<Shell> {
        self.window
    }

    pub fn tray_backend(&self) -> &TrayBackend {
        &self.tray_backend
    }

    pub fn visual(&self, cx: &TestAppContext) -> VisualTestContext {
        VisualTestContext::from_window(self.window.into(), cx)
    }

    pub fn draw(&self, cx: &mut TestAppContext) {
        AnyWindowHandle::from(self.window)
            .update(cx, |_, window, cx| {
                window.refresh();
                let _ = window.draw(cx);
            })
            .expect("draw test application window");
        cx.run_until_parked();
    }

    pub fn write_settings(&self, cx: &mut TestAppContext, settings: Value) {
        let _group = tracing::info_span!("test-settings-write").entered();
        save_settings(&self.paths, &settings);
        tracing::debug!("settings written");
        // The startup watcher owns the reload operation. Advance its virtual timer.
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        tracing::debug!("settings watcher polled");
    }
}

struct TestHome(Rc<TempDir>);

impl TestHome {
    fn parent(&self) -> PathBuf {
        self.0.path().join("app")
    }

    fn path(&self) -> PathBuf {
        self.parent().join("home")
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        // Shell state saves can still fire on the test clock after this handle drops.
        // A file at the parent path stops queued writes from recreating the home.
        let parent = self.parent();
        let _ = fs::remove_dir_all(&parent);
        let _ = fs::File::create_new(parent);
    }
}

struct BootStarted;

impl Global for BootStarted {}

// Keep the parent file until the GPUI context releases its globals.
struct HomeRoot {
    _dir: Rc<TempDir>,
}

impl Global for HomeRoot {}

fn save_settings(paths: &Paths, settings: &Value) {
    let mut bytes = serde_json::to_vec_pretty(settings).expect("encode test settings");
    bytes.push(b'\n');
    yard_paths::write_atomic(&paths.settings_file(), &bytes).expect("write test settings");
}
