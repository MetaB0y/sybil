from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.api_error_response import ApiErrorResponse
from ...models.proof_job_ack_request import ProofJobAckRequest
from ...models.proof_job_ack_response import ProofJobAckResponse
from typing import cast



def _get_kwargs(
    height: int,
    *,
    body: ProofJobAckRequest,

) -> dict[str, Any]:
    headers: dict[str, Any] = {}


    

    

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/prover/jobs/{height}/ack".format(height=quote(str(height), safe=""),),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | ApiErrorResponse | ProofJobAckResponse | None:
    if response.status_code == 200:
        response_200 = ProofJobAckResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorResponse.from_dict(response.json())



        return response_400

    if response.status_code == 503:
        response_503 = cast(Any, None)
        return response_503

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | ApiErrorResponse | ProofJobAckResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    height: int,
    *,
    client: AuthenticatedClient | Client,
    body: ProofJobAckRequest,

) -> Response[Any | ApiErrorResponse | ProofJobAckResponse]:
    """ POST /v1/prover/jobs/{height}/ack

    Args:
        height (int):
        body (ProofJobAckRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | ProofJobAckResponse]
     """


    kwargs = _get_kwargs(
        height=height,
body=body,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    height: int,
    *,
    client: AuthenticatedClient | Client,
    body: ProofJobAckRequest,

) -> Any | ApiErrorResponse | ProofJobAckResponse | None:
    """ POST /v1/prover/jobs/{height}/ack

    Args:
        height (int):
        body (ProofJobAckRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | ProofJobAckResponse
     """


    return sync_detailed(
        height=height,
client=client,
body=body,

    ).parsed

async def asyncio_detailed(
    height: int,
    *,
    client: AuthenticatedClient | Client,
    body: ProofJobAckRequest,

) -> Response[Any | ApiErrorResponse | ProofJobAckResponse]:
    """ POST /v1/prover/jobs/{height}/ack

    Args:
        height (int):
        body (ProofJobAckRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | ProofJobAckResponse]
     """


    kwargs = _get_kwargs(
        height=height,
body=body,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    height: int,
    *,
    client: AuthenticatedClient | Client,
    body: ProofJobAckRequest,

) -> Any | ApiErrorResponse | ProofJobAckResponse | None:
    """ POST /v1/prover/jobs/{height}/ack

    Args:
        height (int):
        body (ProofJobAckRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | ProofJobAckResponse
     """


    return (await asyncio_detailed(
        height=height,
client=client,
body=body,

    )).parsed
