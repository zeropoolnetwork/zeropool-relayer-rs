// use web3::{
//     contract::{Contract, Options},
//     types::U256,
// };
//
// pub struct EvmBackend {
//     contract: Contract<web3::transports::Http>,
// }
//
// impl EvmBackend {
//     pub fn new() -> Self {
//         let contract = Contract::from_json(
//             web3::transports::Http::new("http://localhost:8545").unwrap(),
//             "0x0000000000000000000000000000000000000000"
//                 .parse()
//                 .unwrap(),
//             include_bytes!("./Pool.json"),
//         );
//
//         Self { contract }
//     }
// }
