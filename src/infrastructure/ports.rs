use crate::domain::{
    InputEndpoint, InputPort, OutputEndpoint, OutputPort, PortGroupId, PortMemberId,
};

pub fn input(port: impl Into<String>) -> InputEndpoint {
    InputEndpoint::Socket(InputPort::new(port))
}

pub fn output(port: impl Into<String>) -> OutputEndpoint {
    OutputEndpoint::Socket(OutputPort::new(port))
}

pub fn input_member(group: impl Into<String>, member: impl Into<String>) -> InputEndpoint {
    InputEndpoint::GroupMember {
        group: PortGroupId::new(group),
        member: PortMemberId::new(member),
    }
}

pub fn output_member(group: impl Into<String>, member: impl Into<String>) -> OutputEndpoint {
    OutputEndpoint::GroupMember {
        group: PortGroupId::new(group),
        member: PortMemberId::new(member),
    }
}

pub fn main_input() -> InputEndpoint {
    input("main")
}

pub fn factor_input() -> InputEndpoint {
    input("factor")
}

pub fn amount_input() -> InputEndpoint {
    input("amount")
}

pub fn mask_input() -> InputEndpoint {
    input("mask")
}

pub fn control_input() -> InputEndpoint {
    input("control")
}

pub fn out_output() -> OutputEndpoint {
    output("out")
}

pub fn inputs_member(member: impl Into<String>) -> InputEndpoint {
    input_member("inputs", member)
}

pub fn streams_member(member: impl Into<String>) -> InputEndpoint {
    input_member("streams", member)
}

pub fn branches_member(member: impl Into<String>) -> OutputEndpoint {
    output_member("branches", member)
}

pub fn routes_member(member: impl Into<String>) -> OutputEndpoint {
    output_member("routes", member)
}
