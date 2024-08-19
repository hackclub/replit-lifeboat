## Deployment

```bash
docker buildx build --platform linux/amd64 -t hackclub/replit-takeout:latest .
docker push hackclub/replit-takeout:latest
kubectl rollout restart deployment replit-takeout
```
