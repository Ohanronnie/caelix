use crate::{
    BoxFuture, Container, Controller, EventHandler, EventHandlerDef, Injectable,
    RegisterableEventHandler, Result, WebSocketGateway,
};
use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    fmt::Debug,
    future::Future,
    sync::Arc,
    time::Instant,
};

type ProviderValue = Arc<dyn Any + Send + Sync>;
type BuildProviderFn =
    Box<dyn for<'a> Fn(&'a Container) -> BoxFuture<'a, crate::Result<ProviderValue>> + Send + Sync>;
type LifecycleFn =
    Box<dyn for<'a> Fn(&'a ProviderValue) -> BoxFuture<'a, crate::Result<()>> + Send + Sync>;

/// Metadata for one resolved provider dependency.
#[derive(Clone, Copy)]
/// Public Caelix type `ProviderDependency`.
pub struct ProviderDependency {
    type_id: TypeId,
    type_name: &'static str,
}

impl ProviderDependency {
    /// Runs the `of` public API operation.
    pub fn of<T: Send + Sync + 'static>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
        }
    }

    pub(crate) fn type_id(&self) -> TypeId {
        self.type_id
    }
}

/// Declares provider dependencies for manual `Injectable` implementations and factories.
/// Builds the explicit dependency list required by a handwritten
/// [`Injectable`](crate::Injectable) implementation.
///
/// Pass zero or more provider types, for example
/// `provider_dependencies![UserRepository, Logger]`. The returned metadata is
/// used during module visibility validation before the provider is constructed.
#[macro_export]
macro_rules! provider_dependencies {
    ($($dependency:ty),* $(,)?) => {
        vec![$($crate::ProviderDependency::of::<$dependency>()),*]
    };
}

/// Public Caelix type `ProviderDef`.
pub struct ProviderDef {
    type_id: TypeId,
    type_name: &'static str,
    dependencies: Vec<ProviderDependency>,
    build: BuildProviderFn,
    init_fn: LifecycleFn,
    bootstrap_fn: LifecycleFn,
    shutdown_fn: LifecycleFn,
}

impl ProviderDef {
    /// Runs the `of` public API operation.
    pub fn of<T: Injectable>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            dependencies: T::dependencies(),
            build: Box::new(|container| {
                Box::pin(async move { Ok(Arc::new(T::create(container).await?) as ProviderValue) })
            }),
            init_fn: Box::new(|value| {
                let value = downcast_provider::<T>(value);
                Box::pin(async move { value?.on_module_init().await })
            }),
            bootstrap_fn: Box::new(|value| {
                let value = downcast_provider::<T>(value);
                Box::pin(async move { value?.on_bootstrap().await })
            }),
            shutdown_fn: Box::new(|value| {
                let value = downcast_provider::<T>(value);
                Box::pin(async move { value?.on_shutdown().await })
            }),
        }
    }

    /// Pre-built provider value for tests and other manual registration paths.
    /// Lifecycle hooks are no-ops (`useValue` semantics).
    pub fn instance<T: Send + Sync + 'static>(value: T) -> Self {
        let value = Arc::new(value) as ProviderValue;
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            dependencies: vec![],
            build: Box::new(move |_| {
                let value = value.clone();
                Box::pin(async move { Ok(value) })
            }),
            init_fn: noop_lifecycle(),
            bootstrap_fn: noop_lifecycle(),
            shutdown_fn: noop_lifecycle(),
        }
    }

    /// Runs the `async_factory` public API operation.
    pub fn async_factory<T, Fut, E>(
        dependencies: Vec<ProviderDependency>,
        factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
        E: Debug + Send + 'static,
    {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            dependencies,
            build: Box::new(move |container| {
                let future = factory(Arc::new(container.clone()));
                Box::pin(async move {
                    let value = future.await.map_err(|err| {
                        crate::exception::startup_error(format!(
                            "async factory failed for {}: {:?}",
                            std::any::type_name::<T>(),
                            err
                        ))
                    })?;
                    Ok(Arc::new(value) as ProviderValue)
                })
            }),
            init_fn: noop_lifecycle(),
            bootstrap_fn: noop_lifecycle(),
            shutdown_fn: noop_lifecycle(),
        }
    }

    /// Runs the `type_id` public API operation.
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }
    /// Runs the `type_name` public API operation.
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    fn assert_registered(&self, container: &Container) -> crate::Result<()> {
        if container.contains_type_id(self.type_id) {
            return Ok(());
        }
        Err(crate::exception::startup_error(format!(
            "missing provider at startup: {} was declared by module metadata but was not registered",
            self.type_name
        )))
    }

    async fn run_lifecycle(
        &self,
        value: &ProviderValue,
        hook: &'static str,
        callback: &LifecycleFn,
    ) -> crate::Result<()> {
        callback(value).await.map_err(|err| {
            crate::exception::startup_error(format!(
                "{hook} failed for {}: {}: {}",
                self.type_name, err.error, err.message
            ))
        })
    }

    async fn run_lifecycle_from_container(
        &self,
        container: &Container,
        hook: &'static str,
        callback: &LifecycleFn,
    ) -> crate::Result<()> {
        let value = container.resolve_erased(self.type_id).ok_or_else(|| crate::exception::startup_error(format!(
            "missing provider during {hook}: {} was declared by module metadata but was not registered", self.type_name
        )))?;
        self.run_lifecycle(&value, hook, callback).await
    }
}

/// Public Caelix type `ProviderOverrides`.
pub struct ProviderOverrides {
    defs: HashMap<TypeId, ProviderDef>,
}
impl ProviderOverrides {
    /// Runs the `new` public API operation.
    pub fn new() -> Self {
        Self {
            defs: HashMap::new(),
        }
    }
    /// Runs the `insert_instance` public API operation.
    pub fn insert_instance<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.defs
            .insert(TypeId::of::<T>(), ProviderDef::instance(value));
        self
    }
    /// Runs the `insert_factory` public API operation.
    pub fn insert_factory<T, Fut, E>(
        mut self,
        dependencies: Vec<ProviderDependency>,
        factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
        E: Debug + Send + 'static,
    {
        self.defs.insert(
            TypeId::of::<T>(),
            ProviderDef::async_factory::<T, Fut, E>(dependencies, factory),
        );
        self
    }
    /// Runs the `insert` public API operation.
    pub fn insert(mut self, def: ProviderDef) -> Self {
        self.defs.insert(def.type_id, def);
        self
    }
    pub(crate) fn into_inner(self) -> HashMap<TypeId, ProviderDef> {
        self.defs
    }
}
impl Default for ProviderOverrides {
    fn default() -> Self {
        Self::new()
    }
}

fn downcast_provider<T: Send + Sync + 'static>(value: &ProviderValue) -> crate::Result<Arc<T>> {
    value.clone().downcast::<T>().map_err(|_| {
        crate::exception::startup_error(format!(
            "type mismatch running lifecycle hook for {}",
            std::any::type_name::<T>()
        ))
    })
}
fn noop_lifecycle() -> LifecycleFn {
    Box::new(|_| Box::pin(async { Ok(()) }))
}

/// Public Caelix type `ControllerDef`.
pub struct ControllerDef {
    /// The `register_fn` value.
    pub register_fn: fn(&mut dyn Any),
    /// The `route_log_fn` value.
    pub route_log_fn: fn(),
    #[cfg(feature = "openapi")]
    pub(crate) openapi_routes_fn: fn() -> &'static [crate::openapi::OpenApiRouteDef],
    provider: ProviderDef,
}
impl ControllerDef {
    /// Runs the `of` public API operation.
    pub fn of<C: Controller + Injectable + 'static>() -> Self {
        let mut provider = ProviderDef::of::<C>();
        for dependency in C::route_dependencies() {
            if !provider
                .dependencies
                .iter()
                .any(|existing| existing.type_id == dependency.type_id)
            {
                provider.dependencies.push(dependency);
            }
        }
        Self {
            register_fn: |any| C::register_routes(any),
            route_log_fn: || crate::log_controller_routes::<C>(),
            #[cfg(feature = "openapi")]
            openapi_routes_fn: || C::openapi_routes(),
            provider,
        }
    }
}

/// Public Caelix type `GatewayDef`.
pub struct GatewayDef {
    /// The `path` value.
    pub path: &'static str,
    /// The `type_id` value.
    pub type_id: TypeId,
    provider: ProviderDef,
    kind: GatewayKind,
}
enum GatewayKind {
    WebSocket {
        resolve_fn: fn(&Container) -> crate::Result<Arc<dyn WebSocketGateway>>,
    },
    SocketIo {
        register_fn: fn(&Container, &dyn Any) -> Result<()>,
    },
}
impl GatewayDef {
    /// Runs the `websocket` public API operation.
    pub fn websocket<G: WebSocketGateway>(path: &'static str) -> Self {
        Self {
            path,
            type_id: TypeId::of::<G>(),
            provider: ProviderDef::of::<G>(),
            kind: GatewayKind::WebSocket {
                resolve_fn: |c| Ok(c.resolve::<G>()? as Arc<dyn WebSocketGateway>),
            },
        }
    }
    #[doc(hidden)]
    pub fn socket_io<G: Injectable>(
        path: &'static str,
        register_fn: fn(&Container, &dyn Any) -> crate::Result<()>,
    ) -> Self {
        Self {
            path,
            type_id: TypeId::of::<G>(),
            provider: ProviderDef::of::<G>(),
            kind: GatewayKind::SocketIo { register_fn },
        }
    }
    /// Runs the `resolve` public API operation.
    pub fn resolve(&self, container: &Container) -> Result<Arc<dyn WebSocketGateway>> {
        match self.kind {
            GatewayKind::WebSocket { resolve_fn } => resolve_fn(container),
            GatewayKind::SocketIo { .. } => Err(crate::exception::startup_error(format!(
                "Socket.IO gateway {} cannot be mounted by an RFC 6455 application",
                self.path
            ))),
        }
    }
    #[doc(hidden)]
    pub fn is_websocket(&self) -> bool {
        matches!(self.kind, GatewayKind::WebSocket { .. })
    }
    #[doc(hidden)]
    pub fn register_socket_io(&self, container: &Container, handle: &dyn Any) -> Result<()> {
        match self.kind {
            GatewayKind::WebSocket { .. } => Ok(()),
            GatewayKind::SocketIo { register_fn } => register_fn(container, handle),
        }
    }
}
/// Public Caelix extension trait `Gateway`.
pub trait Gateway: Injectable {
    #[doc(hidden)]
    fn definition() -> GatewayDef;
}

/// Public Caelix type `ModuleDef`.
pub struct ModuleDef {
    type_id: TypeId,
    type_name: &'static str,
    metadata_fn: fn() -> ModuleMetadata,
    pub(crate) route_log_fn: fn(),
}
impl ModuleDef {
    /// Runs the `of` public API operation.
    pub fn of<M: Module + 'static>() -> Self {
        Self {
            type_id: TypeId::of::<M>(),
            type_name: std::any::type_name::<M>(),
            metadata_fn: M::register,
            route_log_fn: || crate::log_module_routes::<M>(),
        }
    }
}
/// Public Caelix extension trait `Module`.
pub trait Module {
    /// Public Caelix API.
    fn register() -> ModuleMetadata;
}

/// Public Caelix type `ModuleMetadata`.
pub struct ModuleMetadata {
    /// The `imports` value.
    pub imports: Vec<ModuleDef>,
    /// The `providers` value.
    pub providers: Vec<ProviderDef>,
    /// The `controllers` value.
    pub controllers: Vec<ControllerDef>,
    /// The `event_handlers` value.
    pub event_handlers: Vec<EventHandlerDef>,
    /// The `gateways` value.
    pub gateways: Vec<GatewayDef>,
    exports: Vec<ProviderDependency>,
    global: bool,
}
impl ModuleMetadata {
    /// Runs the `new` public API operation.
    pub fn new() -> Self {
        Self {
            imports: vec![],
            providers: vec![],
            controllers: vec![],
            event_handlers: vec![],
            gateways: vec![],
            exports: vec![],
            global: false,
        }
    }
    /// Creates metadata for a global module. Only explicitly exported providers become global.
    pub fn global() -> Self {
        let mut metadata = Self::new();
        metadata.global = true;
        metadata
    }
    /// Runs the `import` public API operation.
    pub fn import<M: Module + 'static>(mut self) -> Self {
        self.imports.push(ModuleDef::of::<M>());
        self
    }
    /// Runs the `provider` public API operation.
    pub fn provider<T: Injectable>(mut self) -> Self {
        self.providers.push(ProviderDef::of::<T>());
        self
    }
    /// Runs the `provider_async_factory` public API operation.
    pub fn provider_async_factory<T, Fut, E>(
        mut self,
        dependencies: Vec<ProviderDependency>,
        factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
        E: Debug + Send + 'static,
    {
        self.providers.push(ProviderDef::async_factory::<T, Fut, E>(
            dependencies,
            factory,
        ));
        self
    }
    /// Runs the `controller` public API operation.
    pub fn controller<C: Controller + Injectable + 'static>(mut self) -> Self {
        self.controllers.push(ControllerDef::of::<C>());
        self
    }
    /// Runs the `gateway` public API operation.
    pub fn gateway<G: Gateway>(mut self) -> Self {
        self.gateways.push(G::definition());
        self
    }
    /// Runs the `event_handler` public API operation.
    pub fn event_handler<H>(mut self) -> Self
    where
        H: RegisterableEventHandler + EventHandler<H::Event>,
    {
        self.event_handlers.push(EventHandlerDef::of::<H>());
        self
    }
    /// Runs the `event_handler_for` public API operation.
    pub fn event_handler_for<E, H>(mut self) -> Self
    where
        E: Clone + Send + Sync + 'static,
        H: Injectable + EventHandler<E>,
    {
        self.event_handlers
            .push(EventHandlerDef::for_event::<E, H>());
        self
    }
    /// Makes a locally declared provider, or a direct import's export, available to importing modules.
    pub fn export<T: Send + Sync + 'static>(mut self) -> Self {
        self.exports.push(ProviderDependency::of::<T>());
        self
    }
}
impl Default for ModuleMetadata {
    fn default() -> Self {
        Self::new()
    }
}

struct ModuleNode {
    type_id: TypeId,
    type_name: &'static str,
    metadata: ModuleMetadata,
    imports: Vec<usize>,
}
struct ModuleGraph {
    nodes: Vec<ModuleNode>,
}
#[derive(Clone, Copy)]
enum ProviderSlot {
    Provider(usize),
    Controller(usize),
    Gateway(usize),
}
#[derive(Clone, Copy)]
struct ProviderRegistration {
    module: usize,
    slot: ProviderSlot,
}

impl ModuleGraph {
    fn discover<M: Module + 'static>() -> crate::Result<Self> {
        Self::discover_from(ModuleDef::of::<M>())
    }
    fn discover_from(root: ModuleDef) -> crate::Result<Self> {
        fn visit(
            def: ModuleDef,
            graph: &mut Vec<ModuleNode>,
            states: &mut HashMap<TypeId, u8>,
            path: &mut Vec<&'static str>,
        ) -> crate::Result<usize> {
            match states.get(&def.type_id).copied() {
                Some(2) => {
                    return Ok(graph
                        .iter()
                        .position(|node| node.type_id == def.type_id)
                        .expect("discovered module missing"));
                }
                Some(1) => {
                    let mut cycle = path.clone();
                    cycle.push(def.type_name);
                    return Err(crate::exception::startup_error(format!(
                        "circular module import: {}",
                        cycle.join(" -> ")
                    )));
                }
                _ => {}
            }
            states.insert(def.type_id, 1);
            path.push(def.type_name);
            let metadata = (def.metadata_fn)();
            let index = graph.len();
            graph.push(ModuleNode {
                type_id: def.type_id,
                type_name: def.type_name,
                metadata,
                imports: vec![],
            });
            let imports = std::mem::take(&mut graph[index].metadata.imports);
            for import in imports {
                let child = visit(import, graph, states, path)?;
                graph[index].imports.push(child);
            }
            path.pop();
            states.insert(def.type_id, 2);
            Ok(index)
        }
        let mut nodes = vec![];
        let mut states = HashMap::new();
        let root_index = visit(root, &mut nodes, &mut states, &mut vec![])?;
        // DFS creates parents before children; reverse for imported-first deterministic traversal.
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        fn order(
            index: usize,
            nodes: &Vec<ModuleNode>,
            seen: &mut HashSet<usize>,
            ordered: &mut Vec<usize>,
        ) {
            if !seen.insert(index) {
                return;
            }
            for &child in &nodes[index].imports {
                order(child, nodes, seen, ordered);
            }
            ordered.push(index);
        }
        order(root_index, &nodes, &mut seen, &mut ordered);
        let mut remap = HashMap::new();
        for (new, old) in ordered.iter().enumerate() {
            remap.insert(*old, new);
        }
        let mut slots: Vec<Option<ModuleNode>> = nodes.into_iter().map(Some).collect();
        let mut result = Vec::with_capacity(slots.len());
        for old in ordered {
            let mut node = slots[old]
                .take()
                .expect("module graph ordering repeated a node");
            node.imports = node.imports.into_iter().map(|i| remap[&i]).collect();
            result.push(node);
        }
        let _ = remap[&root_index];
        Ok(Self { nodes: result })
    }

    fn definitions(&self) -> Vec<ProviderRegistration> {
        let mut values = vec![];
        for (module, node) in self.nodes.iter().enumerate() {
            values.extend(
                (0..node.metadata.providers.len()).map(|slot| ProviderRegistration {
                    module,
                    slot: ProviderSlot::Provider(slot),
                }),
            );
            values.extend(
                (0..node.metadata.controllers.len()).map(|slot| ProviderRegistration {
                    module,
                    slot: ProviderSlot::Controller(slot),
                }),
            );
            for slot in 0..node.metadata.gateways.len() {
                values.push(ProviderRegistration {
                    module,
                    slot: ProviderSlot::Gateway(slot),
                });
            }
        }
        values
    }
    fn def(&self, registration: ProviderRegistration) -> &ProviderDef {
        match registration.slot {
            ProviderSlot::Provider(index) => {
                &self.nodes[registration.module].metadata.providers[index]
            }
            ProviderSlot::Controller(index) => {
                &self.nodes[registration.module].metadata.controllers[index].provider
            }
            ProviderSlot::Gateway(index) => {
                &self.nodes[registration.module].metadata.gateways[index].provider
            }
        }
    }
    fn local_types(&self, module: usize) -> HashSet<TypeId> {
        self.definitions()
            .into_iter()
            .filter(|r| r.module == module)
            .map(|r| self.def(r).type_id)
            .collect()
    }
    /// Validates the module graph and returns providers in construction order.
    ///
    /// Passing `None` performs metadata-only validation. A container is only
    /// needed during application startup, when provider overrides and values
    /// registered by application setup may satisfy declarations.
    fn preflight(&self, container: Option<&Container>) -> crate::Result<Vec<ProviderRegistration>> {
        let definitions = self.definitions();
        let mut by_type = HashMap::new();
        for registration in &definitions {
            let def = self.def(*registration);
            if let Some(existing) = by_type.insert(def.type_id, *registration) {
                if !container.is_some_and(|container| {
                    container.has_pending_override(def.type_id)
                        || container.was_overridden(def.type_id)
                }) {
                    return Err(crate::exception::startup_error(format!(
                        "duplicate provider registration for {} in {} and {}",
                        def.type_name,
                        self.nodes[existing.module].type_name,
                        self.nodes[registration.module].type_name
                    )));
                }
            }
        }
        let locals: Vec<HashSet<TypeId>> =
            (0..self.nodes.len()).map(|i| self.local_types(i)).collect();
        let mut exports = vec![HashSet::new(); self.nodes.len()];
        for index in 0..self.nodes.len() {
            let node = &self.nodes[index];
            for export in &node.metadata.exports {
                let imported = node
                    .imports
                    .iter()
                    .any(|&child| exports[child].contains(&export.type_id));
                if !locals[index].contains(&export.type_id) && !imported {
                    return Err(crate::exception::startup_error(format!(
                        "module {} cannot export {}: it is neither declared locally nor exported by a direct import",
                        node.type_name, export.type_name
                    )));
                }
                exports[index].insert(export.type_id);
            }
        }
        let global_exports: HashSet<TypeId> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.metadata.global)
            .flat_map(|(index, _)| exports[index].iter().copied())
            .collect();
        let visible = |module: usize, type_id: TypeId| {
            locals[module].contains(&type_id)
                || global_exports.contains(&type_id)
                || self.nodes[module]
                    .imports
                    .iter()
                    .any(|&child| exports[child].contains(&type_id))
        };
        for registration in &definitions {
            let production = self.def(*registration);
            let effective = container
                .and_then(|container| container.pending_override(production.type_id))
                .unwrap_or(production);
            for dependency in &effective.dependencies {
                if dependency.type_id == TypeId::of::<crate::Logger>() {
                    continue;
                }
                if by_type.contains_key(&dependency.type_id) {
                    if !visible(registration.module, dependency.type_id) {
                        return Err(crate::exception::startup_error(format!(
                            "{} depends on {} but it is not visible in module {}; import and export its module",
                            effective.type_name,
                            dependency.type_name,
                            self.nodes[registration.module].type_name
                        )));
                    }
                } else if !container
                    .is_some_and(|container| container.contains_type_id(dependency.type_id))
                {
                    return Err(crate::exception::startup_error(format!(
                        "missing provider at startup: {} depends on {} but no provider is registered",
                        effective.type_name, dependency.type_name
                    )));
                }
            }
        }
        for (module, node) in self.nodes.iter().enumerate() {
            for handler in &node.metadata.event_handlers {
                handler.assert_registered_or_declared(&locals[module])?;
                if !visible(module, TypeId::of::<crate::EventBus>()) {
                    return Err(crate::exception::startup_error(format!(
                        "no provider registered for EventBus in module {}; import EventModule",
                        node.type_name
                    )));
                }
            }
        }
        let mut sorted = vec![];
        let mut states = HashMap::new();
        let mut stack = vec![];
        fn schedule(
            reg: ProviderRegistration,
            graph: &ModuleGraph,
            by_type: &HashMap<TypeId, ProviderRegistration>,
            container: Option<&Container>,
            states: &mut HashMap<TypeId, u8>,
            stack: &mut Vec<&'static str>,
            sorted: &mut Vec<ProviderRegistration>,
        ) -> crate::Result<()> {
            let def = graph.def(reg);
            match states.get(&def.type_id).copied() {
                Some(2) => return Ok(()),
                Some(1) => {
                    let mut cycle = stack.clone();
                    cycle.push(def.type_name);
                    return Err(crate::exception::startup_error(format!(
                        "provider dependency cycle: {}",
                        cycle.join(" -> ")
                    )));
                }
                _ => {}
            }
            states.insert(def.type_id, 1);
            stack.push(def.type_name);
            let effective = container
                .and_then(|container| container.pending_override(def.type_id))
                .unwrap_or(def);
            for dep in &effective.dependencies {
                if let Some(next) = by_type.get(&dep.type_id) {
                    schedule(*next, graph, by_type, container, states, stack, sorted)?;
                }
            }
            stack.pop();
            states.insert(def.type_id, 2);
            sorted.push(reg);
            Ok(())
        }
        for registration in definitions {
            schedule(
                registration,
                self,
                &by_type,
                container,
                &mut states,
                &mut stack,
                &mut sorted,
            )?;
        }
        Ok(sorted)
    }
    fn def_by_type(&self, type_id: TypeId) -> Option<&ProviderDef> {
        self.definitions().into_iter().find_map(|r| {
            let def = self.def(r);
            (def.type_id == type_id).then_some(def)
        })
    }
}

async fn initialize_graph(graph: &ModuleGraph, container: &mut Container) -> crate::Result<()> {
    let order = graph.preflight(Some(container))?;
    for registration in order {
        let declared = graph.def(registration);
        container.mark_provider_declared(declared.type_id);
        if container.contains_type_id(declared.type_id)
            || container.was_overridden(declared.type_id)
        {
            continue;
        }
        let start = Instant::now();
        let override_def = container.take_pending_override(declared.type_id);
        if override_def.is_some() {
            container.mark_override_applied(declared.type_id);
        }
        let effective = override_def.as_ref().unwrap_or(declared);
        let scoped_container =
            container.scoped_for_provider(effective.type_name, &effective.dependencies);
        let value = match (effective.build)(&scoped_container).await {
            Ok(value) => value,
            Err(error) => {
                rollback_graph(graph, container).await;
                return Err(error);
            }
        };
        container.register_erased(effective.type_id, value.clone());
        if let Err(error) = effective
            .run_lifecycle(&value, "on_module_init", &effective.init_fn)
            .await
        {
            rollback_graph(graph, container).await;
            return Err(error);
        }
        if override_def.is_none() {
            container.record_initialized_provider(declared.type_id);
        }
        crate::log_provider_initialized(effective.type_name, start.elapsed());
    }
    for node in &graph.nodes {
        for handler in &node.metadata.event_handlers {
            if let Err(error) = handler.register(container) {
                rollback_graph(graph, container).await;
                return Err(error);
            }
        }
        crate::log_module_initialized(node.type_name, std::time::Duration::ZERO);
    }
    Ok(())
}

async fn bootstrap_graph(graph: &ModuleGraph, container: &Container) -> crate::Result<()> {
    let order = graph.preflight(Some(container))?;
    let initialized: HashSet<TypeId> = container.initialized_provider_types().into_iter().collect();
    for registration in order {
        let def = graph.def(registration);
        if initialized.contains(&def.type_id)
            && !container.was_overridden(def.type_id)
            && container.begin_provider_bootstrap(def.type_id)
        {
            if let Err(error) = def
                .run_lifecycle_from_container(container, "on_bootstrap", &def.bootstrap_fn)
                .await
            {
                rollback_graph(graph, container).await;
                return Err(error);
            }
        }
    }
    Ok(())
}

async fn rollback_graph(graph: &ModuleGraph, container: &Container) {
    for type_id in container.take_initialized_providers().into_iter().rev() {
        if let Some(def) = graph.def_by_type(type_id) {
            let _ = def
                .run_lifecycle_from_container(container, "on_shutdown", &def.shutdown_fn)
                .await;
        }
    }
}

/// Runs the `register_module` public API operation.
pub async fn register_module<M: Module + 'static>(container: &mut Container) -> Result<()> {
    let graph = ModuleGraph::discover::<M>()?;
    initialize_graph(&graph, container).await
}
/// Runs the `bootstrap_module` public API operation.
pub async fn bootstrap_module<M: Module + 'static>(container: &Container) -> Result<()> {
    let graph = ModuleGraph::discover::<M>()?;
    bootstrap_graph(&graph, container).await
}
/// Runs the `shutdown_module` public API operation.
pub async fn shutdown_module<M: Module + 'static>(container: &Container) -> Result<()> {
    let graph = ModuleGraph::discover::<M>()?;
    let mut first = None;
    for type_id in container.take_initialized_providers().into_iter().rev() {
        if let Some(def) = graph.def_by_type(type_id) {
            if let Err(error) = def
                .run_lifecycle_from_container(container, "on_shutdown", &def.shutdown_fn)
                .await
            {
                if first.is_none() {
                    first = Some(error);
                }
            }
        }
    }
    first.map_or(Ok(()), Err)
}

/// Runs the `build_container` public API operation.
pub async fn build_container<M: Module + 'static>() -> Result<Container> {
    build_container_with_overrides::<M>(ProviderOverrides::new()).await
}
#[doc(hidden)]
pub async fn build_container_with_setup<M: Module + 'static>(
    setup: impl FnOnce(&mut Container),
) -> Result<Container> {
    crate::log_application_starting();
    let mut container = Container::new();
    setup(&mut container);
    if let Err(error) = register_module::<M>(&mut container).await {
        return Err(error);
    }
    if let Err(error) =
        validate_module_providers::<M>(&container).and_then(|_| validate_gateway_paths::<M>())
    {
        let graph = ModuleGraph::discover::<M>()?;
        rollback_graph(&graph, &container).await;
        return Err(error);
    }
    if let Err(error) = bootstrap_module::<M>(&container).await {
        return Err(error);
    }
    Ok(container)
}
/// Runs the `build_container_with_overrides` public API operation.
pub async fn build_container_with_overrides<M: Module + 'static>(
    overrides: ProviderOverrides,
) -> crate::Result<Container> {
    crate::log_application_starting();
    let mut container = Container::new();
    container.seed_overrides(overrides);
    if let Err(error) = register_module::<M>(&mut container).await {
        return Err(error);
    }
    if let Err(error) = container
        .assert_no_unused_overrides()
        .and_then(|_| validate_module_providers::<M>(&container))
        .and_then(|_| validate_gateway_paths::<M>())
    {
        let graph = ModuleGraph::discover::<M>()?;
        rollback_graph(&graph, &container).await;
        return Err(error);
    }
    if let Err(error) = bootstrap_module::<M>(&container).await {
        return Err(error);
    }
    Ok(container)
}

fn validate_gateway_paths_in_graph(graph: &ModuleGraph) -> crate::Result<()> {
    let mut paths = HashSet::new();
    for node in &graph.nodes {
        for gateway in &node.metadata.gateways {
            if !gateway.path.starts_with('/') {
                return Err(crate::exception::startup_error(format!(
                    "websocket gateway path must start with '/': {}",
                    gateway.path
                )));
            }
            if !paths.insert((gateway.path, gateway.is_websocket())) {
                return Err(crate::exception::startup_error(format!(
                    "duplicate gateway path: {}",
                    gateway.path
                )));
            }
        }
    }
    Ok(())
}

fn validate_gateway_paths<M: Module + 'static>() -> crate::Result<()> {
    validate_gateway_paths_in_graph(&ModuleGraph::discover::<M>()?)
}

/// Validates a module's metadata without constructing providers or starting an application.
///
/// This checks module imports, provider declarations and dependencies, exports,
/// event-handler declarations, and WebSocket gateway paths. It intentionally
/// does not invoke provider constructors or factories, lifecycle hooks, external
/// services, or network listeners.
pub fn validate_module<M: Module + 'static>() -> Result<()> {
    let graph = ModuleGraph::discover::<M>()?;
    graph.preflight(None)?;
    validate_gateway_paths_in_graph(&graph)
}

/// Runs the `validate_module_providers` public API operation.
pub fn validate_module_providers<M: Module + 'static>(container: &Container) -> Result<()> {
    let graph = ModuleGraph::discover::<M>()?;
    graph.preflight(Some(container))?;
    for registration in graph.definitions() {
        graph.def(registration).assert_registered(container)?;
    }
    Ok(())
}

/// Runs the `register_module_controllers` public API operation.
pub fn register_module_controllers<M: Module + 'static>(any: &mut dyn Any) {
    if let Ok(graph) = ModuleGraph::discover::<M>() {
        for node in graph.nodes {
            for controller in &node.metadata.controllers {
                (controller.register_fn)(any);
            }
        }
    }
}
#[cfg(feature = "openapi")]
#[doc(hidden)]
pub fn visit_module_openapi_routes<M: Module + 'static>(
    visitor: &mut impl FnMut(&crate::openapi::OpenApiRouteDef),
) {
    if let Ok(graph) = ModuleGraph::discover::<M>() {
        for node in graph.nodes {
            for controller in &node.metadata.controllers {
                for route in (controller.openapi_routes_fn)() {
                    visitor(route);
                }
            }
        }
    }
}
/// Runs the `visit_module_gateways` public API operation.
pub fn visit_module_gateways<M: Module + 'static>(visitor: &mut impl FnMut(&GatewayDef)) {
    if let Ok(graph) = ModuleGraph::discover::<M>() {
        let mut seen = HashSet::new();
        for node in graph.nodes {
            for gateway in &node.metadata.gateways {
                if seen.insert(gateway.type_id) {
                    visitor(gateway);
                }
            }
        }
    }
}
