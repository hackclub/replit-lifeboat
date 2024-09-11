![replit-takeout](https://github.com/user-attachments/assets/e6a26fee-3b23-4732-b16e-b766508948c5)

## Contributors
* Thank you to [PotentialStyx](https://github.com/potentialstyx) for 🚢ing the history exporting & git reconstruction parts of Lifeboat!

## Deployment

```bash
docker buildx build --platform linux/amd64 -t hackclub/replit-takeout:latest .
docker push hackclub/replit-takeout:latest
kubectl rollout restart deployment replit-takeout
```
