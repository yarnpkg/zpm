module.exports = {
  testEnvironment: `node`,
  testTimeout: 50000,
  transform: {
    "\\.[jt]sx?$": require.resolve(`./setup-ts-jest.js`),
  },
  modulePathIgnorePatterns: [`pkg-tests-fixtures`],
  setupFilesAfterEnv: [require.resolve(`./yarn.setup.ts`)],
};
