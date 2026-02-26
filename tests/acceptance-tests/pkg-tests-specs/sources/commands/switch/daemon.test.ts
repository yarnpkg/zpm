describe(`Commands`, () => {
  describe(`switch daemon`, () => {
    test(
      `it should list daemons (empty initially)`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        // First kill all daemons to ensure clean state
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // List should show no daemons
        const result = await runSwitch(`switch`, `daemon`);
        expect(result.code).toBe(0);
        expect(result.stdout).toContain(`No live daemons found`);
      }),
    );

    test(
      `it should list daemons as JSON`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        // First kill all daemons to ensure clean state
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // List as JSON should return empty array
        const result = await runSwitch(`switch`, `daemon`, `--json`);
        expect(result.code).toBe(0);
        const daemons = JSON.parse(result.stdout);
        expect(daemons).toEqual([]);
      }),
    );

    test(
      `it should start a daemon for the current project`,
      makeTemporaryEnv({}, async ({path, runSwitch, yarnBinary}) => {
        // Kill all daemons first
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // Link the actual test yarn binary (which has the daemon command)
        await runSwitch(`switch`, `link`, yarnBinary);

        // Start daemon
        const startResult = await runSwitch(`switch`, `daemon`, `--start`);
        expect(startResult.code).toBe(0);
        expect(startResult.stdout).toContain(`Started daemon`);
        expect(startResult.stdout).toContain(`PID:`);

        // Verify daemon appears in list
        const listResult = await runSwitch(`switch`, `daemon`, `--json`);
        expect(listResult.code).toBe(0);
        const daemons = JSON.parse(listResult.stdout);
        expect(daemons.length).toBe(1);
        expect(typeof daemons[0].pid).toBe(`number`);

        // Clean up
        await runSwitch(`switch`, `daemon`, `--kill-all`);
        await runSwitch(`switch`, `unlink`);
      }),
    );

    test(
      `it should warn when daemon is already running`,
      makeTemporaryEnv({}, async ({path, runSwitch, yarnBinary}) => {
        // Kill all daemons first
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // Link the actual test yarn binary
        await runSwitch(`switch`, `link`, yarnBinary);

        // Start daemon
        await runSwitch(`switch`, `daemon`, `--start`);

        // Try to start again
        const secondStart = await runSwitch(`switch`, `daemon`, `--start`);
        expect(secondStart.code).toBe(0);
        expect(secondStart.stdout).toContain(`already running`);

        // Clean up
        await runSwitch(`switch`, `daemon`, `--kill-all`);
        await runSwitch(`switch`, `unlink`);
      }),
    );

    test(
      `it should kill daemon for current project`,
      makeTemporaryEnv({}, async ({path, runSwitch, yarnBinary}) => {
        // Kill all daemons first
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // Link the actual test yarn binary
        await runSwitch(`switch`, `link`, yarnBinary);

        // Start daemon
        await runSwitch(`switch`, `daemon`, `--start`);

        // Kill it
        const killResult = await runSwitch(`switch`, `daemon`, `--kill`);
        expect(killResult.code).toBe(0);
        expect(killResult.stdout).toContain(`Stopped daemon`);

        // Verify no daemons
        const listResult = await runSwitch(`switch`, `daemon`, `--json`);
        expect(listResult.code).toBe(0);
        const daemons = JSON.parse(listResult.stdout);
        expect(daemons).toEqual([]);

        // Clean up
        await runSwitch(`switch`, `unlink`);
      }),
    );

    test(
      `it should kill all daemons`,
      makeTemporaryEnv({}, async ({path, runSwitch, yarnBinary}) => {
        // Kill all daemons first
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // Link the actual test yarn binary
        await runSwitch(`switch`, `link`, yarnBinary);

        // Start daemon
        await runSwitch(`switch`, `daemon`, `--start`);

        // Kill all
        const killResult = await runSwitch(`switch`, `daemon`, `--kill-all`);
        expect(killResult.code).toBe(0);

        // Verify no daemons
        const listResult = await runSwitch(`switch`, `daemon`, `--json`);
        expect(listResult.code).toBe(0);
        const daemons = JSON.parse(listResult.stdout);
        expect(daemons).toEqual([]);

        // Clean up
        await runSwitch(`switch`, `unlink`);
      }),
    );

    test(
      `it should handle kill with no running daemon`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        // Kill all daemons first
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // Try to kill when none is running
        const killResult = await runSwitch(`switch`, `daemon`, `--kill`);
        expect(killResult.code).toBe(0);
        expect(killResult.stdout).toContain(`No daemon`);
      }),
    );

    test(
      `it should send ping and receive pong`,
      makeTemporaryEnv({}, async ({path, runSwitch, yarnBinary}) => {
        // Kill all daemons first
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // Link the actual test yarn binary
        await runSwitch(`switch`, `link`, yarnBinary);

        // Start daemon
        await runSwitch(`switch`, `daemon`, `--start`);

        // Send ping message
        const sendResult = await runSwitch(`switch`, `daemon`, `--send`, `{"type":"ping"}`);
        expect(sendResult.code).toBe(0);
        const response = JSON.parse(sendResult.stdout);
        expect(response.type).toBe(`pong`);

        // Clean up
        await runSwitch(`switch`, `daemon`, `--kill-all`);
        await runSwitch(`switch`, `unlink`);
      }),
    );

    test(
      `it should error when sending to non-running daemon`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        // Kill all daemons first
        await runSwitch(`switch`, `daemon`, `--kill-all`);

        // Try to send when no daemon is running - should throw
        await expect(runSwitch(`switch`, `daemon`, `--send`, `{"type":"ping"}`)).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringContaining(`No daemon is running`),
        });
      }),
    );
  });
});
