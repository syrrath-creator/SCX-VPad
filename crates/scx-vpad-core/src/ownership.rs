//! Slot ownership is independent of report contents. Owner IDs must come from the
//! authenticated service session, never an untrusted client-supplied field.
use super::{Profile, MAX_DEVICES};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceKey {
    pub slot: u8,
    pub generation: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryError {
    InvalidOwner,
    UnsupportedProfile,
    Full,
    UnknownDevice,
    WrongOwner,
}
#[derive(Clone, Copy)]
struct Slot {
    owner: u64,
    generation: u32,
    profile: Profile,
}
const EMPTY: Slot = Slot {
    owner: 0,
    generation: 0,
    profile: Profile::GenericHid,
};
pub struct DeviceRegistry {
    slots: [Slot; MAX_DEVICES],
}
impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}
impl DeviceRegistry {
    pub const fn new() -> Self {
        Self {
            slots: [EMPTY; MAX_DEVICES],
        }
    }
    pub fn create(
        &mut self,
        authenticated_owner: u64,
        profile: Profile,
    ) -> Result<DeviceKey, RegistryError> {
        if authenticated_owner == 0 {
            return Err(RegistryError::InvalidOwner);
        }
        if !profile.is_supported() {
            return Err(RegistryError::UnsupportedProfile);
        }
        for (index, slot) in self.slots.iter_mut().enumerate() {
            // Exhausted generations retire the slot instead of reviving stale handles.
            if slot.owner == 0 && slot.generation != u32::MAX {
                slot.owner = authenticated_owner;
                slot.generation += 1;
                slot.profile = profile;
                return Ok(DeviceKey {
                    slot: index as u8,
                    generation: slot.generation,
                });
            }
        }
        Err(RegistryError::Full)
    }
    pub fn check(
        &self,
        authenticated_owner: u64,
        key: DeviceKey,
    ) -> Result<Profile, RegistryError> {
        if authenticated_owner == 0 {
            return Err(RegistryError::InvalidOwner);
        }
        let slot = self
            .slots
            .get(key.slot as usize)
            .ok_or(RegistryError::UnknownDevice)?;
        if slot.owner == 0 || slot.generation != key.generation {
            return Err(RegistryError::UnknownDevice);
        }
        if slot.owner != authenticated_owner {
            return Err(RegistryError::WrongOwner);
        }
        Ok(slot.profile)
    }
    pub fn release(
        &mut self,
        authenticated_owner: u64,
        key: DeviceKey,
    ) -> Result<(), RegistryError> {
        self.check(authenticated_owner, key)?;
        self.slots[key.slot as usize].owner = 0;
        Ok(())
    }
    pub fn release_owner(&mut self, authenticated_owner: u64) -> usize {
        if authenticated_owner == 0 {
            return 0;
        }
        let mut released = 0;
        for slot in &mut self.slots {
            if slot.owner == authenticated_owner {
                slot.owner = 0;
                released += 1;
            }
        }
        released
    }
    pub fn count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.owner != 0).count()
    }
    pub fn count_owner(&self, owner: u64) -> usize {
        if owner == 0 {
            return 0;
        }
        self.slots.iter().filter(|slot| slot.owner == owner).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sixteen_slots_are_bounded_and_independently_owned() {
        let mut r = DeviceRegistry::new();
        let mut keys = [DeviceKey {
            slot: 0,
            generation: 0,
        }; 16];
        for (i, key) in keys.iter_mut().enumerate() {
            *key = r.create(i as u64 + 1, Profile::GenericHid).unwrap();
        }
        assert_eq!(r.count(), 16);
        assert_eq!(r.create(17, Profile::GenericHid), Err(RegistryError::Full));
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(r.check(i as u64 + 1, *key), Ok(Profile::GenericHid));
            assert_eq!(
                r.release(i as u64 + 99, *key),
                Err(RegistryError::WrongOwner)
            );
        }
    }
    #[test]
    fn generation_prevents_stale_device_reuse() {
        let mut r = DeviceRegistry::new();
        let a = r.create(1, Profile::GenericHid).unwrap();
        r.release(1, a).unwrap();
        let b = r.create(1, Profile::GenericHid).unwrap();
        assert_eq!(a.slot, b.slot);
        assert_ne!(a.generation, b.generation);
        assert_eq!(r.check(1, a), Err(RegistryError::UnknownDevice));
        assert_eq!(r.check(1, b), Ok(Profile::GenericHid));
    }
    #[test]
    fn disconnect_only_releases_authenticated_owners_devices() {
        let mut r = DeviceRegistry::new();
        r.create(1, Profile::GenericHid).unwrap();
        r.create(1, Profile::GenericHid).unwrap();
        let other = r.create(2, Profile::GenericHid).unwrap();
        assert_eq!(r.release_owner(1), 2);
        assert_eq!(r.count(), 1);
        assert_eq!(r.check(2, other), Ok(Profile::GenericHid));
        assert_eq!(r.release_owner(0), 0);
    }
    #[test]
    fn no_zero_owner_unsupported_profiles_or_generation_wrap() {
        let mut r = DeviceRegistry::new();
        assert_eq!(
            r.create(0, Profile::GenericHid),
            Err(RegistryError::InvalidOwner)
        );
        assert_eq!(
            r.create(1, Profile::Steam),
            Err(RegistryError::UnsupportedProfile)
        );
        r.slots.fill(Slot {
            generation: u32::MAX,
            ..EMPTY
        });
        assert_eq!(r.create(1, Profile::GenericHid), Err(RegistryError::Full));
    }
}
