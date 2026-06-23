// Migrations are an early feature in Anchor and currently just a quick way to
// deploy a program and run setup. Left as the default no-op.
const anchor = require("@coral-xyz/anchor");

module.exports = async function (provider) {
  anchor.setProvider(provider);
};
