// Serialization fixtures computed with protoc/Python from manufacturer schema commit
// 7158570563bdcd6de356fff62e3f8c159189a6d2. No generated vendor code is included.
const RPC_STOP_VECTORS: &[(u32, StopCommand, &str, &str)] = &[
    (1u32, StopCommand::CurrentMode, "080112051001da2c00", "080322020801"),
    (1u32, StopCommand::Hdsp, "080112051001d22e00", "080322020801"),
    (1u32, StopCommand::Hsp, "080112051001823600", "0803223f080182363a0a38080220ffffffffffffffffff012885ffffffffffffffff013d0000803f50ffffffff0f58ffffffffffffffffff0170017d0000803e800101"),
    (127u32, StopCommand::CurrentMode, "08011205107fda2c00", "08032202087f"),
    (127u32, StopCommand::Hdsp, "08011205107fd22e00", "08032202087f"),
    (127u32, StopCommand::Hsp, "08011205107f823600", "0803223f087f82363a0a38080220ffffffffffffffffff012885ffffffffffffffff013d0000803f50ffffffff0f58ffffffffffffffffff0170017d0000803e800101"),
    (128u32, StopCommand::CurrentMode, "08011206108001da2c00", "08032203088001"),
    (128u32, StopCommand::Hdsp, "08011206108001d22e00", "08032203088001"),
    (128u32, StopCommand::Hsp, "08011206108001823600", "0803224008800182363a0a38080220ffffffffffffffffff012885ffffffffffffffff013d0000803f50ffffffff0f58ffffffffffffffffff0170017d0000803e800101"),
    (16384u32, StopCommand::CurrentMode, "0801120710808001da2c00", "0803220408808001"),
    (16384u32, StopCommand::Hdsp, "0801120710808001d22e00", "0803220408808001"),
    (16384u32, StopCommand::Hsp, "0801120710808001823600", "080322410880800182363a0a38080220ffffffffffffffffff012885ffffffffffffffff013d0000803f50ffffffff0f58ffffffffffffffffff0170017d0000803e800101"),
    (2147483648u32, StopCommand::CurrentMode, "08011209108080808008da2c00", "08032206088080808008"),
    (2147483648u32, StopCommand::Hdsp, "08011209108080808008d22e00", "08032206088080808008"),
    (2147483648u32, StopCommand::Hsp, "08011209108080808008823600", "0803224308808080800882363a0a38080220ffffffffffffffffff012885ffffffffffffffff013d0000803f50ffffffff0f58ffffffffffffffffff0170017d0000803e800101"),
    (4294967295u32, StopCommand::CurrentMode, "0801120910ffffffff0fda2c00", "0803220608ffffffff0f"),
    (4294967295u32, StopCommand::Hdsp, "0801120910ffffffff0fd22e00", "0803220608ffffffff0f"),
    (4294967295u32, StopCommand::Hsp, "0801120910ffffffff0f823600", "0803224308ffffffff0f82363a0a38080220ffffffffffffffffff012885ffffffffffffffff013d0000803f50ffffffff0f58ffffffffffffffffff0170017d0000803e800101"),
];
