use super::types::{ActionBarVm, ActionId};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportBucket {
    Wide,
    Medium,
    Compact,
    TouchCompact,
}

pub fn project_for_viewport(mut vm: ActionBarVm, bucket: ViewportBucket) -> ActionBarVm {
    match bucket {
        | ViewportBucket::Wide => vm,
        | ViewportBucket::Medium => {
            vm.overflow.append(&mut vm.contextual);
            vm
        }
        | ViewportBucket::Compact => {
            vm.overflow.append(&mut vm.contextual);
            if let Some(index) = vm.primary.iter().position(|action| action.id == ActionId::Reduce)
            {
                vm.overflow.push(vm.primary.remove(index));
            }
            vm
        }
        | ViewportBucket::TouchCompact => {
            vm.overflow.append(&mut vm.contextual);
            vm.overflow.append(&mut vm.primary);
            vm
        }
    }
}
