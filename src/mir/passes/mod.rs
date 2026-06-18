use crate::mir::MIR;

pub trait MIRPass {
    fn run(&self, mir: &MIR) -> MIR;
}
