import express from 'express';

const app = express();

app.get('/', (_req, res) => {
  res.send('Hello from Yarn with node_modules');
});

app.listen(3000);
