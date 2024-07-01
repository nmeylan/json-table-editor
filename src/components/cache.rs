
pub trait CacheTrait {
    fn update(&mut self);

    fn len(&self) -> usize;

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
pub struct FrameCache<Value, Computer> {
    generation: u32,
    computer: Computer,
    cache: nohash_hasher::IntMap<u64, (u32, Value)>,
}

impl<Value, Computer> Default for FrameCache<Value, Computer>
where
    Computer: Default,
{
    fn default() -> Self {
        Self::new(Computer::default())
    }
}
impl<Value: 'static, Computer: 'static> CacheTrait for FrameCache<Value, Computer> {
    fn update(&mut self) {
        self.evice_cache();
    }

    fn len(&self) -> usize {
        self.cache.len()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl<Value, Computer> FrameCache<Value, Computer> {
    pub fn new(computer: Computer) -> Self {
        Self {
            generation: 0,
            computer,
            cache: Default::default(),
        }
    }

    /// Must be called once per frame to clear the cache.
    pub fn evice_cache(&mut self) {
        let current_generation = self.generation;
        self.cache.retain(|_key, cached| {
            cached.0 == current_generation // only keep those that were used this frame
        });
        self.generation = self.generation.wrapping_add(1);
    }
}

impl<Value, Computer> FrameCache<Value, Computer> {
    /// Get from cache (if the same key was used last frame)
    /// or recompute and store in the cache.
    pub fn get<Key, Param>(&mut self, key: Key, param: Param) -> Value
    where
        Key: Copy + std::hash::Hash,
        Value: Clone,
        Computer: ComputerMut<Key, Param, Value>,
    {
        let hash = egui::util::hash(key);

        match self.cache.entry(hash) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                let cached = entry.into_mut();
                cached.0 = self.generation;
                cached.1.clone()
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let value = self.computer.compute(key, param);
                entry.insert((self.generation, value.clone()));
                value
            }
        }
    }
}
pub trait ComputerMut<Key, Param, Value> {
    fn compute(&mut self, key: Key, param: Param) -> Value;
}

#[derive(Default)]
pub struct CacheStorage {
    caches: egui::ahash::HashMap<std::any::TypeId, Box<dyn CacheTrait>>,
}

impl CacheStorage {
    pub fn cache<FrameCache: CacheTrait + Default + 'static>(&mut self) -> &mut FrameCache {
        self.caches
            .entry(std::any::TypeId::of::<FrameCache>())
            .or_insert_with(|| Box::<FrameCache>::default())
            .as_any_mut()
            .downcast_mut::<FrameCache>()
            .unwrap()
    }

    /// Total number of cached values
    fn num_values(&self) -> usize {
        self.caches.values().map(|cache| cache.len()).sum()
    }

    /// Call once per frame to evict cache.
    pub fn update(&mut self) {
        for cache in self.caches.values_mut() {
            cache.update();
        }
    }
}